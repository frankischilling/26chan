-- Report challenge consumption shares the existing admission transaction.
SET ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before report challenges'; END IF;
END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report snapshot catalog drifted before report challenges'; END IF;
END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before report challenges'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF (SELECT encode(sha256(convert_to(replace(prosrc,chr(13),''),'UTF8')),'hex') FROM pg_proc WHERE oid='content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure) IS DISTINCT FROM '52351d4e9f2eb95dac3654827a46c4e8ef5b88ed798e65f81c41a9b70a98be11' THEN RAISE EXCEPTION 'Report admission changed before challenge enforcement'; END IF; END $$;
DO $$ BEGIN IF (SELECT encode(sha256(convert_to(replace(prosrc,chr(13),''),'UTF8')),'hex') FROM pg_proc WHERE oid='content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure) IS DISTINCT FROM '68d884098b6022118c5a84a9d855a874538851cf3aaed5b3521aa85947bbbf06' THEN RAISE EXCEPTION 'Report admission changed before challenge enforcement'; END IF; END $$;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION content.consume_report_challenge(p_board text,p_post bigint,p_token bytea,
 p_network bytea,p_address bytea,p_environment bytea,p_request bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE invoker text; enabled boolean; allow_credits boolean; raw text; proof jsonb; key text;
 receipt verification_private.receipts%ROWTYPE; profile record;
BEGIN
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 raw:=current_setting('board.challenge_context',true);
 PERFORM set_config('board.challenge_context','',true);
 IF invoker='board_migrator' THEN RETURN; END IF;
 IF invoker<>'board_public' THEN RAISE EXCEPTION 'Report challenge authority is unavailable.' USING ERRCODE='42501'; END IF;
 SELECT b.captcha_enabled,b.captcha_twister AND b.captcha_allow_bypass AND b.slug NOT IN('pol','trash','bant')
 INTO enabled,allow_credits FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 IF NOT enabled THEN RETURN; END IF;
 IF raw IS NULL OR raw='' OR octet_length(raw)>1024 THEN
  RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166'; END IF;
 BEGIN proof:=raw::jsonb;
 EXCEPTION WHEN invalid_text_representation THEN
  RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END;
 IF jsonb_typeof(proof)<>'object' THEN RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(proof))<>4 THEN RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 FOREACH key IN ARRAY ARRAY['receipt','peer','session','context'] LOOP
  IF jsonb_typeof(proof->key) IS DISTINCT FROM 'string' OR octet_length(proof->>key)<>64 OR (proof->>key) !~ '^[0-9a-f]{64}$' THEN
   RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 END LOOP;
 IF p_token IS NULL OR decode(proof->>'session','hex')<>p_token THEN
  RAISE EXCEPTION 'Challenge session changed.' USING ERRCODE='P0166'; END IF;
 receipt:=verification_private.take_write_receipt_data(decode(proof->>'receipt','hex'),decode(proof->>'peer','hex'),p_token,
  'report',p_board,p_post,decode(proof->>'context','hex'));
 IF receipt.from_credit THEN
  IF NOT allow_credits OR NOT (SELECT current_score>=80 FROM content.verification_risk(receipt.peer_hash,NULL)) THEN
   RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
  SELECT * INTO profile FROM verification_private.credit_profile(p_token,p_network,p_address,p_environment,p_request);
  IF NOT profile.can_spend OR profile.identity_key<>receipt.credit_identity THEN
   RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
  UPDATE verification_private.credits SET remaining=remaining-1
   WHERE identity_key=profile.identity_key AND remaining>0 AND expires_at>clock_timestamp();
  IF NOT FOUND THEN RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
 END IF;
END $$;
REVOKE ALL ON FUNCTION content.consume_report_challenge(text,bigint,bytea,bytea,bytea,bytea,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.consume_report_challenge(text,bigint,bytea,bytea,bytea,bytea,bigint) TO board_report_admission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA content TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
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
    -- CAPTCHA precedes the source's report quota/restriction checks. Keeping
    -- consumption here also restores the proof if a later admission step fails.
    PERFORM content.consume_report_challenge(p_board,p_post,p_token,p_network,p_address,p_environment,p_request_at);
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
    -- CAPTCHA precedes the source's report quota/restriction checks. Keeping
    -- consumption here also restores the proof if a later admission step fails.
    PERFORM content.consume_report_challenge(p_board,p_post,p_token,p_network,p_address,p_environment,p_request_at);
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
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION content.public_challenge_ready() RETURNS boolean
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT count(*)=6 AND bool_and(
  p.proowner='board_anonymous_owner'::regrole
  AND p.prosecdef=(p.proname IN('check_public_post_challenge','check_post_challenge_completion'))
  AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner))
  FROM pg_proc p WHERE p.oid IN(
   to_regprocedure('content.check_public_post_challenge()'),
   to_regprocedure('content.check_post_challenge_completion()'),
   to_regprocedure('verification_private.finish_post_challenge(bigint,bytea,bytea,bytea,bytea,bigint)'),
   to_regprocedure('verification_private.take_write_receipt(bytea,bytea,bytea,text,text,bigint,bytea)'),
   to_regprocedure('verification_private.credit_profile(bytea,bytea,bytea,bytea,bigint)'),
   to_regprocedure('verification_private.take_write_receipt_data(bytea,bytea,bytea,text,text,bigint,bytea)'))),false)
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid=to_regprocedure('content.consume_report_challenge(text,bigint,bytea,bytea,bytea,bytea,bigint)')
  AND p.proowner='board_anonymous_owner'::regrole AND p.prosecdef
  AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND has_function_privilege('board_report_admission_owner',p.oid,'EXECUTE')
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE a.privilege_type='EXECUTE' AND a.grantee NOT IN(p.proowner,'board_report_admission_owner'::regrole)))
 AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='content.posts'::regclass
  AND t.tgname='a00_public_post_challenge' AND t.tgfoid='content.check_public_post_challenge()'::regprocedure
  AND t.tgenabled='O' AND t.tgtype=7 AND NOT t.tgdeferrable AND NOT t.tgisinternal
  AND t.tgnargs=0 AND t.tgqual IS NULL)
 AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='content.posts'::regclass
  AND t.tgname='post_challenge_completion' AND t.tgfoid='content.check_post_challenge_completion()'::regprocedure
  AND t.tgenabled='O' AND t.tgtype=5 AND t.tgdeferrable AND t.tginitdeferred AND NOT t.tgisinternal
  AND t.tgnargs=0 AND t.tgqual IS NULL)
 AND NOT EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_verifier','board_mailer']) runtime(role_name)
  WHERE pg_has_role(runtime.role_name,'board_anonymous_owner','MEMBER')
   OR has_schema_privilege(runtime.role_name,'verification_private','USAGE')
   OR has_table_privilege(runtime.role_name,'verification_private.post_handoffs','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))
$$;
REVOKE ALL ON FUNCTION content.public_challenge_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.public_challenge_ready() TO board_public,board_verifier;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
SET ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report challenges did not preserve restriction authority'; END IF; END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report challenges did not preserve snapshot authority'; END IF; END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report challenges did not preserve transfer authority'; END IF; END $$;
RESET ROLE;
