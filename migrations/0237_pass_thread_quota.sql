-- Count live OPs by the validated Pass identity across addresses and sessions.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass active-thread quota'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass active-thread quota'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass active-thread quota'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass active-thread quota'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass active-thread quota'; END IF; END $$;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
RESET ROLE;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.check_user_thread_quota(bytea,text,bigint,bytea,boolean,bigint)'::regprocedure
 AND proowner='board_posting_cooldown_owner'::regrole AND prosecdef
 AND proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]
 AND encode(sha256(convert_to(prosrc,'UTF8')),'hex')='13110d30f1b28b020a26fe5190feeb3c007b0886741699eb1809ca9b7b3da211') THEN
 RAISE EXCEPTION 'Active-thread quota drifted before Pass matching'; END IF;
END $$;
GRANT USAGE ON SCHEMA verification_private TO board_posting_cooldown_owner;
SET LOCAL ROLE board_anonymous_owner;
GRANT SELECT ON verification_private.pass_posts TO board_posting_cooldown_owner;
GRANT EXECUTE ON FUNCTION verification_private.pass_posting_identity(text,bigint,bytea) TO board_posting_cooldown_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
SET LOCAL ROLE board_posting_cooldown_owner;
CREATE OR REPLACE FUNCTION content.check_user_thread_quota(
    p_actor bytea,p_board text,p_request_at bigint,
    p_token bytea,p_minted boolean,p_session_request_at bigint
) RETURNS TABLE(rejected boolean,user_thread_limit integer,user_thread_period_hours integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_invoker text; v_limit integer; v_period integer; v_count bigint;
    v_automatic_identity uuid; v_source_new boolean; v_pass_identity bytea;
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_request_at IS NULL
        OR p_request_at NOT BETWEEN 0 AND 9223372036854689407 THEN
        RAISE EXCEPTION 'Invalid thread quota context.' USING ERRCODE='23514';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Thread quota is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Thread quota requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Preserve 0092's board visibility and quota serialization. The caller
    -- holds actor -> OP capacity -> board -> content admission -> session;
    -- both the board and session locks below are reentries, not new gates.
    SELECT b.user_thread_limit,b.user_thread_period_hours INTO v_limit,v_period
        FROM content.boards b WHERE b.slug=p_board
            AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator'))
        FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002'; END IF;

    -- Read the pre-registration state without allocating an identity or
    -- advancing activity. Same-creation-second and idle-reset requests are
    -- source-new and therefore omit the source UserPwd branch entirely.
    SELECT a.automatic_identity,a.source_new
        INTO STRICT v_automatic_identity,v_source_new
        FROM post_secrets.resolve_automatic_identity(
            p_token,p_minted,p_session_request_at,true) a;

    -- Pass matching is independent of source-new password/session state.
    -- This repeats receipt/account validation without consuming the receipt.
    IF v_invoker='board_public' THEN
        v_pass_identity:=verification_private.pass_posting_identity(p_board,0,p_token);
    END IF;

    SELECT count(*) INTO v_count FROM (
        SELECT op.id FROM content.posts op
        JOIN content.threads t ON t.board=op.board AND t.id=op.thread_id
        WHERE op.board=p_board AND op.id=op.thread_id AND NOT op.deleted
            AND NOT t.deleted AND t.archived_at IS NULL
            -- Strict lower edge, no upper edge, and stored OP source time.
            AND extract(epoch FROM op.created_at)>p_request_at-v_period::bigint*3600
            AND (EXISTS(SELECT 1 FROM post_secrets.posting_history h
                WHERE h.board=op.board AND h.post_id=op.id AND h.thread_id=op.thread_id
                    AND (h.actor_hash=p_actor
                        OR (NOT v_source_new AND v_automatic_identity IS NOT NULL
                            AND h.automatic_identity=v_automatic_identity)))
                OR (v_pass_identity IS NOT NULL AND EXISTS(
                    SELECT 1 FROM verification_private.pass_posts p
                    WHERE p.post_id=op.id AND p.identity_hash=v_pass_identity)))
        -- The outer OP row counts once even when all identity branches match.
        -- Zero remains a real reject-all maximum.
        LIMIT v_limit
    ) bounded;
    RETURN QUERY SELECT v_count>=v_limit,v_limit,v_period;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION verification_private.pass_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='verification_private' AND c.relname LIKE 'pass\_%' ESCAPE '\'
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname IN('verification_private','content') AND p.proname LIKE 'pass\_%' ESCAPE '\')
   OR (n.nspname='content' AND p.proname='public_pass_ready')
   OR p.oid='content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)'::regprocedure
   OR p.oid='post_secrets.record_current_report_clear(bigint)'::regprocedure
   OR p.oid='post_secrets.check_report_limits(text,bigint,bytea,uuid,timestamp with time zone)'::regprocedure
   OR p.oid='content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)'::regprocedure
   OR p.oid='content.check_user_thread_quota(bytea,text,bigint,bytea,boolean,bigint)'::regprocedure
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,
    verification_private.pass_acl(coalesce(r.relacl,acldefault(CASE WHEN r.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
      pg_get_expr(d.adbin,d.adrelid),verification_private.pass_acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY k.conname)
     FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    verification_private.pass_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(r.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY r.identity,t.tgname)
    FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
   ORDER BY t.tgrelid::regclass::text,t.tgname) FROM pg_trigger t WHERE t.tgfoid IN('verification_private.pass_erase_post_identity()'::regprocedure,'verification_private.pass_capture_report_identity()'::regprocedure)),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),verification_private.pass_acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
    FROM pg_namespace n WHERE n.nspname='verification_private'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed report profile'; END IF; END $$;
