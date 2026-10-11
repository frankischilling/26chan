-- Separate source report restrictions. Preserve report quotas, evidence,
-- category authority and anonymous registration; do not run posting expiry.
DO $report_restriction_preflight$
BEGIN
 EXECUTE 'SET LOCAL ROLE board_restriction_owner';
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction dependencies are not ready before report enforcement';
 END IF;
 PERFORM singleton FROM restrictions.capacity WHERE singleton FOR UPDATE;
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Restriction catalog is unavailable'; END IF;
 EXECUTE 'RESET ROLE';
 EXECUTE 'SET LOCAL ROLE board_report_snapshot_owner';
 IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report capture is not ready before report restrictions'; END IF;
 EXECUTE 'RESET ROLE';
 EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer is not ready before report restrictions'; END IF;
 EXECUTE 'RESET ROLE';
 IF (SELECT encode(sha256(convert_to(replace(prosrc,chr(13),''),'UTF8')),'hex') FROM pg_proc WHERE oid='content.admit_report(text,bigint,text,bytea)'::regprocedure) IS DISTINCT FROM 'ecfe14013f4a9c90dc8048a86716c6ff14f10be02ce1648d0fd3eb5db7b3f398' THEN
  RAISE EXCEPTION 'Report admission definition changed before restrictions';
 END IF;
 IF (SELECT encode(sha256(convert_to(replace(prosrc,chr(13),''),'UTF8')),'hex') FROM pg_proc WHERE oid='content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure) IS DISTINCT FROM '6cf61e738dbefe06a188e1b58fd5101817a6954264619ab7a67315483ba1fcf9' THEN
  RAISE EXCEPTION 'Report admission definition changed before restrictions';
 END IF;
 IF (SELECT encode(sha256(convert_to(replace(prosrc,chr(13),''),'UTF8')),'hex') FROM pg_proc WHERE oid='content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure) IS DISTINCT FROM '7f980c1d37bf622820e0efd98fbe3d3590620ce65941b5c266f8bb04931fb237' THEN
  RAISE EXCEPTION 'Report admission definition changed before restrictions';
 END IF;
END $report_restriction_preflight$;

GRANT CREATE ON SCHEMA content,restrictions TO board_restriction_owner;
SET ROLE board_restriction_owner;
-- Compare this policy with the executed, unchanged report_check_ip. Its
-- clauses differ from posting: password matching is not gated by isNew(),
-- Pass needs both presence and bypass, and address matching is never skipped.
CREATE FUNCTION restrictions.source_report_decision(p_rows jsonb,p_board text,
 p_check boolean,p_pass boolean,p_bypass boolean,p_password boolean)
RETURNS boolean LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF p_rows IS NULL OR jsonb_typeof(p_rows)<>'array' OR jsonb_array_length(p_rows)>1000
    OR p_board IS NULL OR p_check IS NULL OR p_pass IS NULL OR p_bypass IS NULL OR p_password IS NULL THEN
  RAISE EXCEPTION 'Invalid report restriction decision' USING ERRCODE='23514';
 END IF;
 RETURN p_check AND EXISTS(SELECT 1 FROM jsonb_to_recordset(p_rows)
  AS b(key text,active boolean,board text,global boolean)
  WHERE b.active AND (b.global OR b.board=p_board)
    AND (b.key='host' OR (b.key='password' AND p_password)
      OR (b.key='4pass_id' AND p_pass AND p_bypass)));
END $$;

CREATE FUNCTION content.bind_report_restrictions(p_report bytea,p_actor bytea,p_token bytea,
 p_board text,p_post bigint,p_request bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF restrictions.invoker() NOT IN('board_public','board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Report restriction context is unavailable' USING ERRCODE='42501';
 END IF;
 IF p_report IS NULL OR octet_length(p_report)<>32 OR p_actor IS NULL OR octet_length(p_actor)<>32
    OR (p_token IS NOT NULL AND octet_length(p_token)<>32)
    OR p_board IS NULL OR p_post IS NULL
    OR p_request IS NULL OR p_request<=0
    OR abs(p_request::numeric-extract(epoch FROM clock_timestamp())::bigint)>30 THEN
  RAISE EXCEPTION 'Invalid report restriction context' USING ERRCODE='23514';
 END IF;
 -- Before report board/gate/session locks. Hold through commit; evaluate only
 -- after the report target and quota precedence has been resolved.
 -- Bind even unknown board names and zero/negative target numbers:
 -- report_target owns their rejection and checks board policy first.
 -- Binding does not authorize a target.
 PERFORM restrictions.gate(false);
 PERFORM set_config('board.report_restriction_context',jsonb_build_object(
  'xid',pg_current_xact_id()::text,'report',encode(p_report,'hex'),'actor',encode(p_actor,'hex'),
  'token',encode(p_token,'hex'),'board',p_board,'post',p_post,'request',p_request)::text,true);
END $$;

CREATE FUNCTION content.assert_reporting_restrictions(p_board text,p_post bigint,p_report bytea DEFAULT NULL)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_context jsonb; v_actor bytea; v_token bytea; v_automatic uuid; v_rows jsonb; v_count integer;
BEGIN
 IF restrictions.invoker()='board_migrator' THEN RETURN; END IF;
 PERFORM restrictions.gate(false);
 -- Old direct admission fixtures/operators need no identity context when
 -- there are no active restrictions. The held gate prevents issuance racing
 -- that empty-state decision. With any active row omission is fail-closed.
 IF NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE active) THEN RETURN; END IF;
 v_context:=nullif(current_setting('board.report_restriction_context',true),'')::jsonb;
 IF v_context IS NULL OR v_context->>'xid' IS DISTINCT FROM pg_current_xact_id()::text
    OR v_context->>'board' IS DISTINCT FROM p_board
    OR (v_context->>'post')::bigint IS DISTINCT FROM p_post
    OR (p_report IS NOT NULL AND v_context->>'report' IS DISTINCT FROM encode(p_report,'hex')) THEN
  RAISE EXCEPTION 'Report restriction context is unavailable' USING ERRCODE='P135C';
 END IF;
 v_actor:=decode(v_context->>'actor','hex'); v_token:=decode(v_context->>'token','hex');
 IF v_actor IS NULL OR octet_length(v_actor)<>32 OR (v_token IS NOT NULL AND octet_length(v_token)<>32) THEN
  RAISE EXCEPTION 'Invalid report restriction identity' USING ERRCODE='P135C';
 END IF;
 -- Report UserPwd->getPwd() has no !isNew() condition. Retained equality
 -- therefore survives creation-second/idle reset when a valid stored token
 -- exists. Fresh/missing credentials invent no prior equality.
 SELECT a.automatic_identity INTO v_automatic FROM post_secrets.lookup_automatic_identity(
  v_token,(v_context->>'request')::bigint) a;
 WITH matches AS (
  SELECT b.board,b.global,b.active,'host'::text AS key FROM restrictions.bans b WHERE b.active AND b.actor_hash=v_actor
  UNION ALL
  SELECT b.board,b.global,b.active,'password'::text AS key FROM restrictions.bans b
    WHERE b.active AND v_automatic IS NOT NULL AND b.automatic_identity=v_automatic
 ), bounded AS(SELECT * FROM matches LIMIT 1001)
 SELECT count(*),coalesce(jsonb_agg(to_jsonb(b)),'[]'::jsonb) INTO v_count,v_rows FROM bounded b;
 IF v_count>1000 THEN RAISE EXCEPTION 'Report restriction matches exceed bound' USING ERRCODE='P134C'; END IF;
 -- No trusted Pass provider exists. Neither request data nor an automatic
 -- session's verified_level can grant a report Pass/bypass exemption.
 IF restrictions.source_report_decision(v_rows,p_board,true,false,false,v_automatic IS NOT NULL) THEN
  RAISE EXCEPTION 'You cannot report while banned' USING ERRCODE='P135B';
 END IF;
END $$;

CREATE FUNCTION content.check_report_restriction_insert() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)='board_migrator' THEN
  RETURN NEW;
 END IF;
 IF current_user<>'board_report_admission_owner' THEN
  RAISE EXCEPTION 'Report insertion authority is unavailable' USING ERRCODE='42501';
 END IF;
 PERFORM content.assert_reporting_restrictions(NEW.board,NEW.post_id);
 -- A second runtime report in the same transaction needs its own binding.
 PERFORM set_config('board.report_restriction_context','',true);
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION restrictions.source_report_decision(jsonb,text,boolean,boolean,boolean,boolean),
 content.bind_report_restrictions(bytea,bytea,bytea,text,bigint,bigint),
 content.assert_reporting_restrictions(text,bigint,bytea),content.check_report_restriction_insert() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.bind_report_restrictions(bytea,bytea,bytea,text,bigint,bigint)
 TO board_public,board_staff;
GRANT EXECUTE ON FUNCTION content.assert_reporting_restrictions(text,bigint,bytea) TO board_report_admission_owner;
GRANT EXECUTE ON FUNCTION content.check_report_restriction_insert() TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,restrictions FROM board_restriction_owner;
CREATE TRIGGER source_report_restriction_guard BEFORE INSERT ON content.reports
 FOR EACH ROW EXECUTE FUNCTION content.check_report_restriction_insert();

-- Reinstall the reviewed existing admission definitions with one check after
-- quota evaluation. CREATE OR REPLACE preserves ownership and exact grants.
GRANT CREATE ON SCHEMA content TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
CREATE OR REPLACE FUNCTION content.admit_report(p_board text,p_post bigint,p_reason text,p_actor bytea)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_revision bigint;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report admission requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
    END IF;
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid report actor.' USING ERRCODE='22023';
    END IF;
    IF p_reason IS NULL OR octet_length(p_reason) NOT BETWEEN 1 AND 1000 OR btrim(p_reason)='' THEN
        RAISE EXCEPTION 'Report reason must contain 1 to 1000 bytes.' USING ERRCODE='22023';
    END IF;
    -- Board -> singleton gate. Never acquire another board after this gate.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    IF v_revision IS NOT NULL THEN
        RAISE EXCEPTION 'Free-text reporting is not active.' USING ERRCODE='P0001';
    END IF;
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_now);
    PERFORM content.assert_reporting_restrictions(p_board,p_post,p_actor);
    -- Operator safety cap, not source quota: only this private membership is
    -- bounded. No eviction/TTL releases old duplicate protection. Lowering the
    -- cap below current occupancy blocks new reports until retirement or an
    -- operator raises the limit; retained content.reports/audit are unaffected.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership LIMIT v_limit) members)>=v_limit THEN
        RAISE EXCEPTION 'Report admission capacity is exhausted.' USING ERRCODE='P0094';
    END IF;
    INSERT INTO content.reports(board,post_id,reason,created_at)
        VALUES(p_board,p_post,p_reason,v_now) RETURNING id INTO v_report;
    INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
        VALUES(v_report,p_actor,p_board,p_post,v_thread,v_now);
    RETURN v_report;
END $$;

CREATE OR REPLACE FUNCTION content.admit_report(
    p_board text,p_post bigint,p_reason text,p_actor bytea,
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_revision bigint; v_automatic_identity uuid;
    v_known boolean; v_evaluated_at timestamptz;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report admission requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_migrator') THEN
        RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
    END IF;
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid report actor.' USING ERRCODE='22023';
    END IF;
    IF p_reason IS NULL OR octet_length(p_reason) NOT BETWEEN 1 AND 1000 OR btrim(p_reason)='' THEN
        RAISE EXCEPTION 'Report reason must contain 1 to 1000 bytes.' USING ERRCODE='22023';
    END IF;
    -- Board -> singleton report gate -> anonymous session. Never acquire
    -- another board after this gate. Registration reenters this same board.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    IF v_revision IS NOT NULL THEN
        RAISE EXCEPTION 'Free-text reporting is not active.' USING ERRCODE='P0001';
    END IF;
    -- Preserve target-policy precedence over session authorization errors.
    -- Recheck below because the session wait can cross an archive expiry.
    PERFORM post_secrets.report_target(p_board,p_post,clock_timestamp());
    -- Resolve/lock without allocation or activity before capturing admission
    -- time. A wait here can cross a cooldown or an archive expiry boundary.
    SELECT a.automatic_identity INTO STRICT v_automatic_identity
        FROM post_secrets.resolve_automatic_identity(p_token,p_minted,p_request_at,true) a;
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_automatic_identity,v_now);
    PERFORM content.assert_reporting_restrictions(p_board,p_post,p_actor);
    -- Operator safety cap, not source quota: only this private membership is
    -- bounded. No eviction/TTL releases old duplicate protection. Lowering the
    -- cap below current occupancy blocks new reports until retirement or an
    -- operator raises the limit; retained content.reports/audit are unaffected.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership LIMIT v_limit) members)>=v_limit THEN
        RAISE EXCEPTION 'Report admission capacity is exhausted.' USING ERRCODE='P0094';
    END IF;
    -- All admission checks have passed while board -> gate -> session locks
    -- remain held. Observe resumed PRE-report state before registration can
    -- set the pending report bit, change activity, or allocate a fresh session.
    -- Registration already revalidates request freshness through 0095's
    -- resolve_automatic_identity and 0065's advance_anonymous_session. Preserve
    -- that final boundary unchanged; this observation does not extend it.
    v_known:=post_secrets.report_known_or_verified(
        p_token,p_network,p_address,p_environment,p_minted,p_request_at);
    v_evaluated_at:=clock_timestamp();
    INSERT INTO content.reports(board,post_id,reason,created_at)
        VALUES(p_board,p_post,p_reason,v_now) RETURNING id INTO v_report;
    INSERT INTO post_secrets.report_weight_evidence(
        report_id,evaluator_version,known_or_verified,effective_weight,numeric_proof,evaluated_at)
        VALUES(v_report,1,v_known,NULL,NULL,v_evaluated_at);
    INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
        VALUES(v_report,p_actor,p_board,p_post,v_thread,v_now,NULL);
    -- Registration requires current-transaction provenance and a NULL identity,
    -- then allocates/reuses the session UUID and stamps this membership itself.
    -- Any failure rolls the report, membership and session changes back together.
    PERFORM content.register_anonymous_report(
        p_token,p_network,p_address,p_environment,p_minted,p_board,v_report,p_request_at);
    RETURN v_report;
END $$;

CREATE OR REPLACE FUNCTION content.admit_categorical_report(
    p_board text,p_post bigint,p_category bigint,p_expected_revision bigint,p_actor bytea,
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_automatic_identity uuid; v_revision bigint; v_category record;
    v_known boolean; v_evaluated_at timestamptz;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report admission requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_migrator') THEN
        RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
    END IF;
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid report actor.' USING ERRCODE='22023';
    END IF;
    -- Board -> singleton report gate -> anonymous session. Never acquire
    -- another board after this gate. Registration reenters this same board.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    -- Preserve target-policy precedence over session authorization errors.
    -- Recheck below because the session wait can cross an archive expiry.
    PERFORM post_secrets.report_target(p_board,p_post,clock_timestamp());
    -- Resolve/lock without allocation or activity before capturing admission
    -- time. A wait here can cross a cooldown or an archive expiry boundary.
    SELECT a.automatic_identity INTO STRICT v_automatic_identity
        FROM post_secrets.resolve_automatic_identity(p_token,p_minted,p_request_at,true) a;
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_automatic_identity,v_now);
    PERFORM content.assert_reporting_restrictions(p_board,p_post,p_actor);
    -- Operator safety cap, not source quota: only this private membership is
    -- bounded. No eviction/TTL releases old duplicate protection. Lowering the
    -- cap below current occupancy blocks new reports until retirement or an
    -- operator raises the limit; retained content.reports/audit are unaffected.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership LIMIT v_limit) members)>=v_limit THEN
        RAISE EXCEPTION 'Report admission capacity is exhausted.' USING ERRCODE='P0094';
    END IF;
    -- Authoritative mode, revision and eligibility are checked after quotas,
    -- while the gate prevents activation changes until this transaction ends.
    IF v_revision IS NULL THEN
        RAISE EXCEPTION 'Categorical reporting is not active.' USING ERRCODE='P0001';
    END IF;
    IF p_expected_revision IS DISTINCT FROM v_revision THEN
        RAISE EXCEPTION 'Report categories changed. Please reload the report form.' USING ERRCODE='P0001';
    END IF;
    SELECT * INTO v_category FROM post_secrets.eligible_report_categories(p_board,p_post,v_revision)
        WHERE id=p_category;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Invalid category selected.' USING ERRCODE='P0001';
    END IF;
    -- All admission checks have passed while board -> gate -> session locks
    -- remain held. Observe resumed PRE-report state before registration can
    -- set the pending report bit, change activity, or allocate a fresh session.
    -- Registration already revalidates request freshness through 0095's
    -- resolve_automatic_identity and 0065's advance_anonymous_session. Preserve
    -- that final boundary unchanged; this observation does not extend it.
    v_known:=post_secrets.report_known_or_verified(
        p_token,p_network,p_address,p_environment,p_minted,p_request_at);
    v_evaluated_at:=clock_timestamp();
    INSERT INTO content.reports(board,post_id,reason,created_at,
        category_revision,category_id,category_kind,category_base_weight)
        VALUES(p_board,p_post,v_category.title,v_now,
            v_revision,v_category.id,v_category.kind,v_category.base_weight)
        RETURNING id INTO v_report;
    -- Source staff authority remains Unknown even for a privileged database
    -- caller. Only base==fallback proves a number across every possible path;
    -- the source reason remains Unknown even when this numeric value is known.
    INSERT INTO post_secrets.report_weight_evidence(
        report_id,evaluator_version,known_or_verified,effective_weight,numeric_proof,evaluated_at)
        VALUES(v_report,1,v_known,
            CASE WHEN v_category.base_weight=0.5 THEN 0.5::double precision ELSE NULL END,
            CASE WHEN v_category.base_weight=0.5 THEN 'BaseEqualsFallback' ELSE NULL END,
            v_evaluated_at);
    INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
        VALUES(v_report,p_actor,p_board,p_post,v_thread,v_now,NULL);
    -- Registration requires current-transaction provenance and a NULL identity,
    -- then allocates/reuses the session UUID and stamps this membership itself.
    -- Any failure rolls the report, membership and session changes back together.
    PERFORM content.register_anonymous_report(
        p_token,p_network,p_address,p_environment,p_minted,p_board,v_report,p_request_at);
    RETURN v_report;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_report_admission_owner;
GRANT CREATE ON SCHEMA restrictions TO board_restriction_owner;
SET ROLE board_restriction_owner;
CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions'
 ), triggers AS (
  SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'post_secrets.posting_history'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='post_secrets.posting_history'::regclass AND t.tgname='source_restriction_guard' AND NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'content.reports'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='content.reports'::regclass AND t.tgname='source_report_restriction_guard' AND NOT t.tgisinternal
 ), funcs AS (
  SELECT p.* FROM pg_proc p WHERE p.proowner='board_restriction_owner'::regrole
    OR p.oid IN(SELECT t.tgfoid FROM triggers t)
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,restrictions.acl(coalesce(r.relacl,acldefault('r',r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
       pg_get_expr(d.adbin,d.adrelid),restrictions.acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated)
       ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
    ORDER BY t.identity,t.tgname) FROM triggers t),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),pg_get_functiondef(p.oid),
      has_function_privilege('board_restriction_owner',p.oid,'EXECUTE')) ORDER BY p.oid::regprocedure::text)
   FROM pg_proc p WHERE p.oid IN('staff_identity.has_board_access(bigint,text)'::regprocedure,
      'content.lock_posting_actor(bytea,boolean)'::regprocedure,
      'post_secrets.lookup_automatic_identity(bytea,bigint)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
      'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),restrictions.acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
   FROM pg_namespace n WHERE n.nspname='restrictions'))::text,'UTF8'))
$$;

RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions FROM board_restriction_owner;
SET ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report restriction catalog did not qualify'; END IF; END $$;
RESET ROLE;
