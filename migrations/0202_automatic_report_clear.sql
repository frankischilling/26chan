-- Source admin_clear_reports is unweighted and does not record clear credits.
-- Automatic state is separate from each report's retained manual attribution.
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report profile drifted before automatic clearing'; END IF; END $$;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer authority drifted before automatic clearing'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report snapshot authority drifted before automatic clearing'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction authority drifted before automatic clearing'; END IF; END $$;
RESET ROLE;
ALTER TABLE content.reports ADD COLUMN auto_cleared_at timestamptz;
GRANT SELECT(auto_cleared_at),UPDATE(auto_cleared_at) ON content.reports TO board_report_admission_owner;
CREATE INDEX reports_auto_clear_history ON content.reports(board,id DESC)
 WHERE auto_cleared_at IS NOT NULL AND reporter_cleared_at IS NULL;
GRANT CREATE ON SCHEMA post_secrets,content TO board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;
CREATE FUNCTION post_secrets.auto_clear_post_reports(p_board text,p_post bigint) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE ids bigint[]; affected bigint; at_time timestamptz;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Automatic report clearing requires Read Committed' USING ERRCODE='22023'; END IF;
 IF p_board IS NULL OR p_board='' OR p_post IS NULL OR p_post<=0 THEN
  RAISE EXCEPTION 'Invalid automatic report clear target' USING ERRCODE='23514'; END IF;
 IF content.report_profile_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Automatic report clear authority is unavailable' USING ERRCODE='P201U'; END IF;
 -- Issuance must acquire actor/restriction gates and this exclusive board lock
 -- before target locks. NOWAIT refuses a lock-order upgrade, never waits here.
 PERFORM slug FROM content.boards WHERE slug=p_board FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Automatic report clear target is unavailable' USING ERRCODE='P0002'; END IF;
 PERFORM singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable' USING ERRCODE='P0094'; END IF;
 PERFORM post_id FROM post_secrets.report_group WHERE board=p_board AND post_id=p_post FOR UPDATE;
 SELECT array_agg(r.id ORDER BY r.id) INTO ids FROM (
  SELECT id FROM content.reports WHERE board=p_board AND post_id=p_post AND reporter_cleared_at IS NULL ORDER BY id LIMIT 10001
 ) r;
 IF coalesce(cardinality(ids),0)>10000 THEN
  RAISE EXCEPTION 'Automatic report clear limit exceeded' USING ERRCODE='54000'; END IF;
 at_time:=clock_timestamp();
 UPDATE content.reports SET auto_cleared_at=at_time
  WHERE board=p_board AND post_id=p_post AND reporter_cleared_at IS NULL AND id=ANY(coalesce(ids,'{}'::bigint[]));
 GET DIAGNOSTICS affected=ROW_COUNT;
 IF affected<>coalesce(cardinality(ids),0) THEN
  RAISE EXCEPTION 'Automatic report clear target changed' USING ERRCODE='P201U'; END IF;
 -- Source preserves each report's cleared_by. The aggregate Auto-clear label
 -- is projected from this separate marker, never substituted for an actor.
 RETURN affected;
END $$;
REVOKE ALL ON FUNCTION post_secrets.auto_clear_post_reports(text,bigint) FROM PUBLIC;
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT DISTINCT a.grantee FROM pg_proc p
 CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
 WHERE p.oid='post_secrets.auto_clear_post_reports(text,bigint)'::regprocedure AND a.grantee NOT IN(0,p.proowner)
 LOOP EXECUTE format('REVOKE ALL ON FUNCTION post_secrets.auto_clear_post_reports(text,bigint) FROM %I',pg_get_userbyid(r.grantee)); END LOOP;
END $$;
GRANT EXECUTE ON FUNCTION post_secrets.auto_clear_post_reports(text,bigint) TO board_restriction_owner;
CREATE OR REPLACE FUNCTION post_secrets.increment_report_group() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_expected bigint; v_updated bigint;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report group maintenance requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Supported bulk writers pre-lock ALL boards in canonical order before
    -- membership/FK tuple locks. NOWAIT only protects this later reentry.
    -- Admission already holds board -> gate -> session; take no new gate or
    -- session lock here. The transition relation contains successful INSERTs.
    PERFORM b.slug FROM content.boards b
        WHERE EXISTS(SELECT 1 FROM report_group_inserted n WHERE n.board=b.slug)
        ORDER BY b.slug FOR UPDATE OF b NOWAIT;
    IF EXISTS (
        SELECT 1 FROM report_group_inserted n
        LEFT JOIN content.reports r ON r.id=n.report_id
        WHERE r.id IS NULL OR r.board IS DISTINCT FROM n.board
            OR r.post_id IS DISTINCT FROM n.post_id
            OR r.reporter_cleared_at IS NOT NULL
            OR r.group_cleared_at IS NOT NULL OR r.group_cleared_by IS NOT NULL
            OR r.group_clear_inherited IS NOT NULL OR r.auto_cleared_at IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'Report group membership evidence is inconsistent.' USING ERRCODE='23514';
    END IF;
    -- Preserve 0100's persistent counters and historical uncertainty. A new
    -- lifetime defaults to uncleared; the conflict branch deliberately leaves
    -- its existing clear metadata untouched. No retained report seeds it.
    INSERT INTO post_secrets.report_group AS g(board,post_id,illegal_count,incomplete)
        SELECT n.board,n.post_id,count(*) FILTER (WHERE r.category_kind=2),
            bool_or(r.category_kind IS NULL) OR (
                NOT EXISTS(SELECT 1 FROM post_secrets.report_group prior
                    WHERE prior.board=n.board AND prior.post_id=n.post_id)
                AND EXISTS(SELECT 1 FROM post_secrets.report_membership old_member
                    WHERE old_member.board=n.board AND old_member.post_id=n.post_id
                        AND NOT EXISTS(SELECT 1 FROM report_group_inserted added
                            WHERE added.report_id=old_member.report_id)))
        FROM report_group_inserted n
        LEFT JOIN content.reports r ON r.id=n.report_id
        GROUP BY n.board,n.post_id
        ON CONFLICT(board,post_id) DO UPDATE SET
            illegal_count=g.illegal_count+EXCLUDED.illegal_count,
            incomplete=g.incomplete OR EXCLUDED.incomplete;

    -- All three admission overloads insert membership after the report and
    -- before returning. Later session registration failure rolls this back as
    -- well. Inherited reports may have unknown weights: they inherit a proven
    -- prior clear, rather than authorizing a fresh clear or reopening a group.
    SELECT count(*) INTO v_expected FROM report_group_inserted n
        JOIN post_secrets.report_group g ON g.board=n.board AND g.post_id=n.post_id
        WHERE g.cleared_at IS NOT NULL;
    UPDATE content.reports r SET group_cleared_at=g.cleared_at,
        group_cleared_by=g.cleared_by,group_clear_inherited=true
        FROM report_group_inserted n
        JOIN post_secrets.report_group g ON g.board=n.board AND g.post_id=n.post_id
        WHERE r.id=n.report_id AND r.board=n.board AND r.post_id=n.post_id
            AND g.cleared_at IS NOT NULL AND r.reporter_cleared_at IS NULL
            AND r.group_cleared_at IS NULL AND r.group_cleared_by IS NULL
            AND r.group_clear_inherited IS NULL;
    GET DIAGNOSTICS v_updated=ROW_COUNT;
    IF v_updated IS DISTINCT FROM v_expected THEN
        RAISE EXCEPTION 'Inherited report clear count changed.' USING ERRCODE='23514';
    END IF;
    -- Auto-clear is retained per report, including legacy rows without a group.
    -- Source LIMIT 1 has no ordering. Use the oldest retained eligible row as
    -- the native deterministic choice; never inspect reporter-retired rows.
    WITH inherited AS MATERIALIZED (
      SELECT DISTINCT ON(n.board,n.post_id) n.board,n.post_id,
        prior.auto_cleared_at,prior.group_cleared_at,prior.group_cleared_by
      FROM report_group_inserted n JOIN content.reports prior
        ON prior.board=n.board AND prior.post_id=n.post_id
      WHERE prior.reporter_cleared_at IS NULL AND prior.auto_cleared_at IS NOT NULL
        AND NOT EXISTS(SELECT 1 FROM report_group_inserted added WHERE added.report_id=prior.id)
      ORDER BY n.board,n.post_id,prior.id
    )
    UPDATE content.reports r SET auto_cleared_at=i.auto_cleared_at,
      group_cleared_at=i.group_cleared_at,group_cleared_by=i.group_cleared_by,
      group_clear_inherited=CASE WHEN i.group_cleared_at IS NOT NULL THEN true ELSE NULL END
    FROM report_group_inserted n JOIN inherited i ON i.board=n.board AND i.post_id=n.post_id
    WHERE r.id=n.report_id AND r.board=n.board AND r.post_id=n.post_id;
    RETURN NULL;
END $$;
CREATE OR REPLACE FUNCTION content.clear_report_group(p_board text,p_post bigint,p_account bigint) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_invoker text;
    v_ids bigint[];
    v_cleared_at timestamptz;
    v_cleared_by bigint;
    v_now timestamptz;
    v_count bigint;
    v_updated bigint;
    v_known bigint;
    v_weight double precision;
    v_consistent boolean;
    v_actors bytea[]; v_times timestamptz[];
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker<>'board_staff' THEN
        RAISE EXCEPTION 'Report group clear is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report group clear requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF p_board IS NULL OR p_board='' OR p_post IS NULL OR p_post<=0
        OR p_account IS NULL OR p_account<=0 THEN
        RAISE EXCEPTION 'Invalid report group clear target.' USING ERRCODE='22023';
    END IF;
    -- Caller starts a fresh content transaction and verifies board-scoped
    -- janitor-or-higher authority. Only the authenticated server supplies the
    -- actor. Lock the target board, then the shared admission gate, before
    -- group/report tuples. History recording needs no session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
    PERFORM singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    -- Automatic clearing already removed this target from the active queue.
    -- It grants no manual-clear history credit and needs no weight proof.
    IF EXISTS(SELECT 1 FROM content.reports WHERE board=p_board AND post_id=p_post
      AND reporter_cleared_at IS NULL AND auto_cleared_at IS NOT NULL) THEN RETURN 0; END IF;
    -- Fresh RC statements after the wait include committed admission, reporter
    -- purge, and archive/deletion retirement. Never acquire another board here.
    SELECT g.cleared_at,g.cleared_by INTO v_cleared_at,v_cleared_by
        FROM post_secrets.report_group g WHERE g.board=p_board AND g.post_id=p_post
        FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
    SELECT array_agg(m.report_id ORDER BY m.report_id) INTO v_ids FROM (
        SELECT member.report_id FROM post_secrets.report_membership member
        WHERE member.board=p_board AND member.post_id=p_post
        ORDER BY member.report_id LIMIT 10001
    ) m;
    IF v_ids IS NULL THEN RETURN NULL; END IF;
    v_count:=cardinality(v_ids);
    IF v_count>10000 THEN
        RAISE EXCEPTION 'Report group clear limit exceeded.' USING ERRCODE='54000';
    END IF;
    SELECT count(*),bool_and(r.board=p_board AND r.post_id=p_post
            AND r.reporter_cleared_at IS NULL
            AND r.group_cleared_at IS NOT DISTINCT FROM v_cleared_at
            AND r.group_cleared_by IS NOT DISTINCT FROM v_cleared_by
            AND ((v_cleared_at IS NULL AND r.group_clear_inherited IS NULL)
                OR (v_cleared_at IS NOT NULL AND r.group_clear_inherited IS NOT NULL)))
        INTO v_updated,v_consistent FROM content.reports r WHERE r.id=ANY(v_ids);
    IF v_updated IS DISTINCT FROM v_count OR v_consistent IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Report group clear evidence is inconsistent.' USING ERRCODE='23514';
    END IF;
    -- Idempotent retry: unknown-weight admissions may have inherited this
    -- already-proven clear. Never audit or clear them for a second time.
    IF v_cleared_at IS NOT NULL THEN RETURN 0; END IF;
    SELECT count(e.effective_weight),sum(e.effective_weight)
        INTO v_known,v_weight FROM post_secrets.report_weight_evidence e
        WHERE e.report_id=ANY(v_ids);
    -- Do not substitute catalog bases, missing evidence, retired history or
    -- today's session observations. Current 0102 proofs establish only 0.5;
    -- the explicit zero/nonfinite rejection also protects future evaluators.
    IF v_known IS DISTINCT FROM v_count OR v_weight IS NULL OR v_weight=0
        OR v_weight IN ('Infinity'::double precision,'-Infinity'::double precision,'NaN'::double precision) THEN
        RAISE EXCEPTION 'Report group weight is not established for clearing.' USING ERRCODE='P0108';
    END IF;
    v_now:=clock_timestamp();
    SELECT array_agg(m.actor_hash ORDER BY m.actor_hash),array_agg(v_now ORDER BY m.actor_hash) INTO v_actors,v_times
    FROM (SELECT DISTINCT actor_hash FROM post_secrets.report_membership WHERE report_id=ANY(v_ids)) m;
    PERFORM content.record_report_challenge_history(v_actors,v_times,true);
    UPDATE content.reports r SET group_cleared_at=v_now,
        group_cleared_by=p_account,group_clear_inherited=false
        WHERE r.id=ANY(v_ids) AND r.board=p_board AND r.post_id=p_post
            AND r.reporter_cleared_at IS NULL AND r.group_cleared_at IS NULL
            AND r.group_cleared_by IS NULL AND r.group_clear_inherited IS NULL;
    GET DIAGNOSTICS v_updated=ROW_COUNT;
    IF v_updated IS DISTINCT FROM v_count THEN
        RAISE EXCEPTION 'Report group clear count changed.' USING ERRCODE='23514';
    END IF;
    UPDATE post_secrets.report_group SET cleared_at=v_now,cleared_by=p_account
        WHERE board=p_board AND post_id=p_post AND cleared_at IS NULL AND cleared_by IS NULL;
    GET DIAGNOSTICS v_updated=ROW_COUNT;
    IF v_updated<>1 THEN
        RAISE EXCEPTION 'Report group clear lifetime changed.' USING ERRCODE='23514';
    END IF;
    RETURN v_count;
END $$;
CREATE OR REPLACE FUNCTION post_secrets.capture_report_challenge_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE actors bytea[]; times timestamptz[]; latest timestamptz; finite boolean;
BEGIN
 IF TG_TABLE_SCHEMA<>'post_secrets' OR TG_TABLE_NAME<>'report_membership' OR TG_OP<>'INSERT'
    OR TG_LEVEL<>'STATEMENT' OR TG_WHEN<>'AFTER' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid report history trigger.' USING ERRCODE='23514'; END IF;
 SELECT array_agg(n.actor_hash ORDER BY n.report_id),array_agg(n.reported_at ORDER BY n.report_id) INTO actors,times
 FROM (SELECT actor_hash,reported_at,report_id FROM report_challenge_inserted ORDER BY report_id LIMIT 10001) n;
 IF actors IS NULL THEN RETURN NULL; END IF;
 IF cardinality(actors)>10000 THEN
  -- Preserve the existing operator bulk-insert contract. Recording is bounded;
  -- an oversized batch instead disables optional exemption until its events
  -- are outside every report/clear window. The reports still commit normally.
  SELECT max(reported_at),bool_and(isfinite(reported_at)) INTO latest,finite FROM report_challenge_inserted;
  IF finite IS DISTINCT FROM true THEN RAISE EXCEPTION 'Invalid report history time.' USING ERRCODE='23514'; END IF;
  PERFORM content.invalidate_report_challenge_history(latest);
  RETURN NULL;
 END IF;
 PERFORM content.record_report_challenge_history(actors,times,false);
 -- The alphabetically earlier group trigger has already stamped inheritance.
 -- A new report against a cleared group logs a new clear event now, rather
 -- than reusing the group's earlier clear timestamp.
 SELECT array_agg(n.actor_hash ORDER BY n.report_id),array_agg(n.reported_at ORDER BY n.report_id) INTO actors,times
 FROM report_challenge_inserted n JOIN content.reports r ON r.id=n.report_id
 WHERE r.group_clear_inherited IS TRUE OR r.auto_cleared_at IS NOT NULL;
 IF actors IS NOT NULL THEN PERFORM content.record_report_challenge_history(actors,times,true); END IF;
 RETURN NULL;
END $$;
CREATE OR REPLACE FUNCTION post_secrets.report_profile_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
 SET search_path=pg_catalog,pg_temp AS $$
 WITH relations AS (
  SELECT c.* FROM pg_class c WHERE c.oid IN('post_secrets.report_weight_evidence'::regclass,
   'post_secrets.report_profile_catalog'::regclass,'post_secrets.anonymous_sessions'::regclass,
   'post_secrets.report_group'::regclass,'content.reports'::regclass)
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname='content' AND p.proname IN('report_profile_ready','admit_report_profiled','admit_categorical_report_profiled','clear_report_group'))
   OR (n.nspname='post_secrets' AND p.proname IN('report_profile_catalog_hash','report_known_or_verified','auto_clear_post_reports','increment_report_group','capture_report_challenge_history'))
   OR p.oid IN('content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
    'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(c.oid::regclass::text,pg_get_userbyid(c.relowner),c.relkind,
   c.relpersistence,c.relrowsecurity,c.relforcerowsecurity,c.reloptions,coalesce(c.relacl,acldefault('r',c.relowner))::text,
   (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
    pg_get_expr(d.adbin,d.adrelid),a.attacl::text) ORDER BY a.attnum)
    FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
   (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY k.conname)
    FROM pg_constraint k WHERE k.conrelid=c.oid),
   (SELECT jsonb_agg(jsonb_build_array(pg_get_indexdef(i.indexrelid),i.indisvalid,i.indisready) ORDER BY i.indexrelid::regclass::text)
    FROM pg_index i WHERE i.indrelid=c.oid),
   (SELECT jsonb_agg(jsonb_build_array(t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY t.tgname)
    FROM pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal)) ORDER BY c.oid::regclass::text) FROM relations c),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
   coalesce(p.proacl,acldefault('f',p.proowner))::text,pg_get_functiondef(p.oid)) ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,
    t.tgtype,t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
    ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(SELECT oid FROM pg_constraint
    WHERE conrelid='post_secrets.report_weight_evidence'::regclass AND contype='f')),
  (SELECT jsonb_agg(jsonb_build_array(nspname,pg_get_userbyid(nspowner),coalesce(nspacl,acldefault('n',nspowner))::text) ORDER BY nspname)
   FROM pg_namespace WHERE nspname IN('post_secrets','content'))
 )::text,'UTF8'))
$$;
CREATE OR REPLACE FUNCTION content.report_profile_ready() RETURNS boolean LANGUAGE sql STABLE
 SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT definition_hash=post_secrets.report_profile_catalog_hash()
  FROM post_secrets.report_profile_catalog WHERE singleton),false)
 AND NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname IN('board_report_admission_owner','board_anonymous_owner','board_restriction_owner')
  AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member IN('board_report_admission_owner'::regrole,'board_anonymous_owner'::regrole,'board_restriction_owner'::regrole))
 AND NOT EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth','board_verifier',
  'board_media','board_media_read','board_media_intake','board_monitor','board_mailer','board_moderation_retention')
  AND (pg_has_role(r.oid,'board_report_admission_owner'::regrole,'MEMBER') OR pg_has_role(r.oid,'board_anonymous_owner'::regrole,'MEMBER') OR pg_has_role(r.oid,'board_restriction_owner'::regrole,'MEMBER')))
 AND NOT EXISTS(SELECT 1 FROM (VALUES ('board_report_admission_owner'::regrole),('board_anonymous_owner'::regrole),('board_restriction_owner'::regrole)) AS required(owner_id)
  WHERE NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.roleid=required.owner_id
   AND m.member='board_migrator'::regrole AND NOT m.admin_option AND NOT m.inherit_option AND m.set_option))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid IN('board_report_admission_owner'::regrole,'board_anonymous_owner'::regrole,'board_restriction_owner'::regrole)
  AND (member<>'board_migrator'::regrole OR admin_option OR inherit_option OR NOT set_option))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets,content FROM board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_report_profile_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_report_profile_hash'),'hex') WHERE singleton;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Automatic report clear profile is unavailable'; END IF; END $$;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report snapshot authority changed around automatic clearing'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer authority changed around automatic clearing'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction authority changed around automatic clearing'; END IF; END $$;
RESET ROLE;
