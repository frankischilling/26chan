-- Apply trusted report network bypass after quotas and explicit bans.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass report network policy'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass report network policy'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass report network policy'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass report network policy'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass report network policy'; END IF; END $$;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION content.pass_report_network_bypass(p_actor bytea,p_anonymous bytea,p_board text,p_post bigint)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 PERFORM verification_private.pass_gate();
 RETURN coalesce((SELECT NOT b.captcha_enabled FROM content.boards b WHERE b.slug=p_board),false)
 OR EXISTS(SELECT 1 FROM verification_private.pass_report_handoffs h
 WHERE h.registration_xid=pg_current_xact_id() AND h.actor_hash=p_actor
 AND h.session_hash=p_anonymous AND h.board=p_board AND h.post_id=p_post);
END $$;
REVOKE ALL ON FUNCTION content.pass_report_network_bypass(bytea,bytea,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_report_network_bypass(bytea,bytea,text,bigint) TO board_report_admission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;
CREATE OR REPLACE FUNCTION post_secrets.assert_report_network(
    p_board text,p_post bigint,p_actor bytea,p_token bytea,p_network bytea,
    p_address bytea,p_environment bytea,p_minted boolean,p_activity_at bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    raw text; ctx jsonb; r record; activity record; have_activity boolean:=false;
    work_safe boolean; request_at bigint:=0; ipv4 bigint; asn bigint; peer_known boolean; asn_known boolean;
    browser_hint text; request_hint text; selected boolean; unknown boolean;
    since_at bigint; network_age bigint; known boolean;
BEGIN
    -- The report owner already has private-board visibility and worksafe read.
    -- The narrow policy owner must not join boards through its public-only RLS.
    SELECT b.worksafe INTO work_safe FROM content.boards b WHERE b.slug=p_board;
    IF NOT FOUND OR work_safe IS NULL THEN
        RAISE EXCEPTION 'Report board policy is unavailable.' USING ERRCODE='P0179'; END IF;
    raw:=current_setting('board.report_network_context',true);
    -- Request-bound comparison data, never CAPTCHA, identity or Pass authority.
    -- Consume the context once even if there are no configured rules.
    PERFORM set_config('board.report_network_context','',true);
    -- Admission has already checked quotas and explicit bans.
    IF content.pass_report_network_bypass(p_actor,p_token,p_board,p_post) THEN RETURN; END IF;
    IF raw IS NOT NULL AND raw<>'' THEN
        IF octet_length(raw)>2048 THEN
            RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179';
        END IF;
        BEGIN
            ctx:=raw::jsonb;
            IF jsonb_typeof(ctx) IS DISTINCT FROM 'object' THEN
                RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
            IF (SELECT count(*) FROM jsonb_object_keys(ctx))<>10
                OR ctx->>'board' IS DISTINCT FROM p_board
                OR ctx->>'post' IS DISTINCT FROM p_post::text
                OR ctx->>'actor' IS DISTINCT FROM encode(p_actor,'hex')
                OR jsonb_typeof(ctx->'request_at') IS DISTINCT FROM 'number'
                OR (ctx->>'request_at') !~ '^[0-9]{1,18}$'
                OR jsonb_typeof(ctx->'peer_known') IS DISTINCT FROM 'boolean'
                OR jsonb_typeof(ctx->'asn_known') IS DISTINCT FROM 'boolean'
                OR NOT (ctx ?& ARRAY['ipv4','asn','browser','request']) THEN
                RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179';
            END IF;
            request_at:=(ctx->>'request_at')::bigint;
            peer_known:=(ctx->>'peer_known')::boolean;
            asn_known:=(ctx->>'asn_known')::boolean;
            IF ctx->'ipv4'<>'null'::jsonb THEN
                IF jsonb_typeof(ctx->'ipv4')<>'number' OR (ctx->>'ipv4') !~ '^[0-9]{1,10}$' THEN
                    RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
                ipv4:=(ctx->>'ipv4')::bigint;
                IF NOT peer_known OR ipv4>4294967295 THEN
                    RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
            END IF;
            IF ctx->'asn'<>'null'::jsonb THEN
                IF jsonb_typeof(ctx->'asn')<>'number' OR (ctx->>'asn') !~ '^[0-9]{1,10}$' THEN
                    RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
                asn:=(ctx->>'asn')::bigint;
                IF NOT asn_known OR asn NOT BETWEEN 1 AND 4294967295 THEN
                    RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
            END IF;
            IF ctx->'browser'<>'null'::jsonb AND jsonb_typeof(ctx->'browser')<>'string'
                OR ctx->'request'<>'null'::jsonb AND jsonb_typeof(ctx->'request')<>'string' THEN
                RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
            browser_hint:=ctx->>'browser'; request_hint:=ctx->>'request';
            IF octet_length(browser_hint)>128 OR browser_hint ~ '[[:cntrl:]]'
                OR octet_length(request_hint)>128 OR request_hint ~ '[[:cntrl:]]' THEN
                RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
        EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
            RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179';
        END;
    END IF;
    -- Missing context uses time zero to conservatively observe all active rules.
    -- Empty policy and posting-only rules retain the compatibility entry points.
    IF (SELECT count(*) FROM content.report_network_rules(p_board,request_at,work_safe))>128 THEN
        RAISE EXCEPTION 'Report network policy is unavailable.' USING ERRCODE='P0179'; END IF;
    FOR r IN SELECT * FROM content.report_network_rules(p_board,request_at,work_safe) LOOP
        IF r.ops_only OR r.images_only THEN CONTINUE; END IF;
        IF ctx IS NULL THEN RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
        selected:=false; unknown:=false;
        IF r.range_start IS NOT NULL THEN
            IF NOT peer_known THEN unknown:=true;
            ELSIF ipv4 BETWEEN r.range_start AND r.range_end THEN selected:=true;
            END IF;
        END IF;
        IF r.asn IS NOT NULL THEN
            IF NOT asn_known THEN unknown:=true;
            ELSIF asn=r.asn THEN selected:=true;
            END IF;
        END IF;
        IF NOT selected THEN
            IF unknown THEN RAISE EXCEPTION 'Report network provider is unavailable.' USING ERRCODE='P0179'; END IF;
            CONTINUE;
        END IF;
        IF r.signatures NOT IN ('','0') THEN
            selected:=(browser_hint IS NOT NULL AND browser_hint NOT IN ('','0') AND strpos(r.signatures,browser_hint)>0)
                OR (request_hint IS NOT NULL AND request_hint NOT IN ('','0') AND strpos(r.signatures,request_hint)>0);
            IF NOT selected THEN
                IF browser_hint IS NULL OR request_hint IS NULL THEN
                    RAISE EXCEPTION 'Report network provider is unavailable.' USING ERRCODE='P0179'; END IF;
                CONTINUE;
            END IF;
        END IF;
        IF r.leniency<>0 AND p_token IS NOT NULL THEN
            IF NOT have_activity THEN
                SELECT * INTO STRICT activity FROM post_secrets.report_network_activity(
                    p_token,p_network,p_address,p_environment,p_minted,p_activity_at);
                have_activity:=true;
            END IF;
            since_at:=CASE WHEN r.updated_at<>0 THEN r.updated_at ELSE r.created_at END;
            network_age:=greatest(0,p_activity_at-activity.network_at);
            known:=NOT (activity.change_score>9 AND network_age<1800) AND (
                network_age>=86400
                OR (since_at>0 AND activity.network_at<=since_at AND (activity.post_count>0 OR activity.report_count>5))
                OR (greatest(0,p_activity_at-activity.created_at)>=86400 AND (
                    since_at>0 OR ((activity.post_count>=3 OR activity.report_count>=10)
                        AND (network_age>=1800 OR activity.post_count>=9 OR activity.report_count>=20)))));
            IF (r.leniency=1 AND (activity.verified OR known))
                OR (r.leniency=2 AND known) OR (r.leniency=3 AND activity.verified) THEN CONTINUE; END IF;
        END IF;
        RAISE EXCEPTION 'Reporting from this IP range has been blocked due to abuse.' USING ERRCODE='P179B';
    END LOOP;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;
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
