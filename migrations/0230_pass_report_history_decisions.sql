-- Private source report-history decisions; incomplete history remains unknown.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before report history decisions'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before report history decisions'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before report history decisions'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before report history decisions'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before report history decisions'; END IF; END $$;
GRANT CREATE ON SCHEMA content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE FUNCTION content.pass_report_abuse_history(p_actor bytea,p_automatic uuid,p_membership bytea,p_now timestamptz)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR (p_membership IS NOT NULL AND octet_length(p_membership)<>32)
 OR p_now IS NULL OR NOT isfinite(p_now) THEN RAISE EXCEPTION 'Invalid report ban history' USING ERRCODE='23514'; END IF;
 PERFORM restrictions.gate(false);
 RETURN (SELECT count(*)>=3 FROM (
  SELECT 1 FROM restrictions.bans b WHERE NOT b.active AND b.template_id=190 AND b.starts_at>p_now-interval '30 days'
   AND ((p_membership IS NOT NULL AND b.pass_identity_hash=p_membership)
    OR (p_membership IS NULL AND b.actor_hash=p_actor)
    OR (p_automatic IS NOT NULL AND b.automatic_identity=p_automatic)) LIMIT 3) matches);
END $$;
REVOKE ALL ON FUNCTION content.pass_report_abuse_history(bytea,uuid,bytea,timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_report_abuse_history(bytea,uuid,bytea,timestamptz) TO board_anonymous_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_restriction_owner;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION content.pass_report_history_filtered(p_board text,p_post bigint,p_actor bytea,p_token bytea,p_request bigint,p_threshold bigint)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE membership bytea; automatic uuid; context jsonb; raw text; stamp timestamptz:=clock_timestamp(); matched bigint;
BEGIN
 IF p_threshold IS NULL THEN RETURN NULL; END IF;
 IF p_threshold<1 THEN RETURN false; END IF;
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_token IS NULL OR octet_length(p_token)<>32
 OR p_board IS NULL OR p_post IS NULL OR p_post<=0 OR p_request IS NULL OR p_request<=0 THEN RETURN NULL; END IF;
 PERFORM verification_private.pass_gate();
 raw:=current_setting('board.report_restriction_context',true);
 IF raw IS NULL OR raw='' OR octet_length(raw)>2048 THEN RETURN NULL; END IF;
 BEGIN context:=raw::jsonb; EXCEPTION WHEN invalid_text_representation THEN RETURN NULL; END;
 IF jsonb_typeof(context) IS DISTINCT FROM 'object'
 OR context->>'xid' IS DISTINCT FROM pg_current_xact_id()::text
 OR context->>'report' IS DISTINCT FROM encode(p_actor,'hex') OR context->>'token' IS DISTINCT FROM encode(p_token,'hex')
 OR context->>'board' IS DISTINCT FROM p_board OR context->>'post' IS DISTINCT FROM p_post::text
 OR context->>'request' IS DISTINCT FROM p_request::text OR coalesce(context->>'actor','') !~ '^[0-9a-f]{64}$' THEN RETURN NULL; END IF;
 SELECT CASE WHEN NOT a.source_new THEN a.automatic_identity END INTO automatic
 FROM post_secrets.lookup_automatic_identity(p_token,p_request) a;
 membership:=verification_private.pass_report_identity(p_actor,p_token,p_board,p_post);
 -- A member's primary identity replaces the IP branch, unlike report bans.
 -- The password branch is a union: overlaps count once, including NULL Pass.
 SELECT count(*) INTO matched FROM verification_private.pass_report_clear_events e
 WHERE e.created_at>stamp-interval '2 days' AND (
  (membership IS NOT NULL AND e.identity_hash=membership)
  OR (membership IS NULL AND e.actor_hash=p_actor)
  OR (automatic IS NOT NULL AND e.automatic_identity=automatic));
 IF matched>=p_threshold THEN RETURN true; END IF;
 IF content.pass_report_abuse_history(decode(context->>'actor','hex'),automatic,membership,stamp) THEN RETURN true; END IF;
 -- Observe rotation before making a negative claim. Missing epochs, omitted
 -- events and migration warm-up preserve Unknown instead of inventing history.
 PERFORM content.observe_report_challenge_epoch();
 IF NOT EXISTS(SELECT 1 FROM verification_private.pass_report_history_policy WHERE singleton AND complete_after<=stamp)
 OR NOT EXISTS(SELECT 1 FROM verification_private.policy WHERE singleton AND report_history_epoch IS NOT NULL
  AND NOT report_history_epoch_tainted AND report_history_complete_after<=stamp) THEN RETURN NULL; END IF;
 RETURN false;
END $$;
REVOKE ALL ON FUNCTION content.pass_report_history_filtered(text,bigint,bytea,bytea,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_report_history_filtered(text,bigint,bytea,bytea,bigint,bigint) TO board_report_admission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
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
