-- Match consumed report membership to bans; public receipt admission remains disconnected.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass report bans'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass report bans'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass report bans'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass report bans'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass report bans'; END IF; END $$;
GRANT CREATE ON SCHEMA verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION verification_private.pass_report_identity(p_actor bytea,p_anonymous bytea,p_board text,p_post bigint)
RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 PERFORM verification_private.pass_gate();
 RETURN (SELECT h.identity_hash FROM verification_private.pass_report_handoffs h
 WHERE h.registration_xid=pg_current_xact_id() AND h.actor_hash=p_actor
 AND h.session_hash=p_anonymous AND h.board=p_board AND h.post_id=p_post);
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_report_identity(bytea,bytea,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION verification_private.pass_report_identity(bytea,bytea,text,bigint) TO board_restriction_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE OR REPLACE FUNCTION content.assert_reporting_restrictions(p_board text,p_post bigint,p_report bytea DEFAULT NULL)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_context jsonb; v_actor bytea; v_token bytea; v_automatic uuid; v_rows jsonb; v_count integer; v_membership bytea;
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
 v_membership:=verification_private.pass_report_identity(decode(v_context->>'report','hex'),v_token,p_board,p_post);
 WITH matches AS (
  SELECT b.board,b.global,b.active,'host'::text AS key FROM restrictions.bans b WHERE b.active AND b.actor_hash=v_actor
  UNION ALL
  SELECT b.board,b.global,b.active,'password'::text AS key FROM restrictions.bans b
    WHERE b.active AND v_automatic IS NOT NULL AND b.automatic_identity=v_automatic
  UNION ALL
  SELECT b.board,b.global,b.active,'4pass_id'::text AS key FROM restrictions.bans b
    WHERE b.active AND v_membership IS NOT NULL AND b.pass_identity_hash=v_membership
 ), bounded AS(SELECT * FROM matches LIMIT 1001)
 SELECT count(*),coalesce(jsonb_agg(to_jsonb(b)),'[]'::jsonb) INTO v_count,v_rows FROM bounded b;
 IF v_count>1000 THEN RAISE EXCEPTION 'Report restriction matches exceed bound' USING ERRCODE='P134C'; END IF;
 -- Reporting retains the host branch even for a validated member.
 IF restrictions.source_report_decision(v_rows,p_board,true,v_membership IS NOT NULL,v_membership IS NOT NULL,v_automatic IS NOT NULL) THEN
  RAISE EXCEPTION 'You cannot report while banned' USING ERRCODE='P135B';
 END IF;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_restriction_owner;
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
