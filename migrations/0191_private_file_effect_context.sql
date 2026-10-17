-- Private cross-owner context for the eventual capability-bound file finalizer.
-- These functions do not grant a posting runtime permission to issue effects.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before file effect context'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before file effect context'; END IF;
END $$;
RESET ROLE;
GRANT USAGE ON SCHEMA admission TO board_restriction_owner;
GRANT CREATE ON SCHEMA admission TO board_admission_owner;
SET LOCAL ROLE board_admission_owner;
CREATE FUNCTION admission.resolve_file_effect_rule(
 p_board text,p_revision bigint,p_normalized bytea,p_original bytea,p_eligible boolean
) RETURNS TABLE(rule_id bigint,digest bytea,mode smallint,ban_days integer,quiet boolean,
 public_reason text,description text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_worksafe boolean; v_revision bigint; candidates bigint[]; matches bigint[]; chosen bytea;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed'
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_revision IS NULL
    OR (p_normalized IS NOT NULL AND octet_length(p_normalized)<>16)
    OR (p_original IS NOT NULL AND octet_length(p_original)<>16) THEN
  RAISE EXCEPTION 'Invalid private file context.' USING ERRCODE='23514'; END IF;
 -- The outer finalizer must already own the actor/gate/board locks.
 SELECT b.worksafe INTO v_worksafe FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 SELECT p.revision INTO v_revision FROM admission.policy p WHERE p.singleton FOR SHARE;
 IF NOT FOUND OR v_revision<>p_revision THEN
  RAISE EXCEPTION 'File admission policy changed.' USING ERRCODE='55000'; END IF;
 SELECT ARRAY(SELECT r.id FROM admission.file_rules r WHERE r.active
  AND (r.board='' OR r.board=p_board OR (v_worksafe AND r.board='_ws_')) ORDER BY r.id LIMIT 129)
 INTO candidates;
 IF cardinality(candidates)>128 THEN
  RAISE EXCEPTION 'File admission policy is unavailable.' USING ERRCODE='55000'; END IF;
 IF cardinality(candidates)=0 THEN RETURN; END IF;
 IF p_normalized IS NULL THEN
  RAISE EXCEPTION 'Normalized file identity is unavailable.' USING ERRCODE='55000'; END IF;
 chosen:=p_normalized;
 SELECT ARRAY(SELECT r.id FROM admission.file_rules r WHERE r.id=ANY(candidates) AND r.digest=chosen)
 INTO matches;
 IF cardinality(matches)=0 THEN
  IF p_eligible IS FALSE THEN RETURN; END IF;
  IF p_original IS NULL THEN
   RAISE EXCEPTION 'Original file identity is unavailable.' USING ERRCODE='55000'; END IF;
  IF p_original=p_normalized THEN RETURN; END IF;
  chosen:=p_original;
  SELECT ARRAY(SELECT r.id FROM admission.file_rules r WHERE r.id=ANY(candidates) AND r.digest=chosen)
  INTO matches;
  IF cardinality(matches)=0 THEN RETURN; END IF;
  IF p_eligible IS NULL THEN
   RAISE EXCEPTION 'Original file eligibility is unavailable.' USING ERRCODE='55000'; END IF;
 END IF;
 IF cardinality(matches)<>1 THEN
  RAISE EXCEPTION 'File admission match is ambiguous.' USING ERRCODE='55000'; END IF;
 RETURN QUERY SELECT r.id,chosen,r.mode,r.ban_days,r.quiet,r.public_reason,r.description
  FROM admission.file_rules r WHERE r.id=matches[1];
END $$;
CREATE FUNCTION admission.file_effect_exempt(p_peer inet) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'File exemptions require read committed isolation.' USING ERRCODE='55000'; END IF;
 PERFORM singleton FROM admission.policy WHERE singleton FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'File admission policy is unavailable.' USING ERRCODE='55000'; END IF;
 RETURN admission.automatic_ban_exempt(p_peer);
END $$;
CREATE FUNCTION admission.file_effect_dmca(p_actor bytea) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 RETURN admission.record_dmca_warning(p_actor,date_trunc('second',clock_timestamp()));
END $$;
REVOKE ALL ON FUNCTION admission.resolve_file_effect_rule(text,bigint,bytea,bytea,boolean),
 admission.file_effect_exempt(inet),admission.file_effect_dmca(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION admission.resolve_file_effect_rule(text,bigint,bytea,bytea,boolean),
 admission.file_effect_exempt(inet),admission.file_effect_dmca(bytea) TO board_restriction_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA admission FROM board_admission_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'File effect context changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'File effect context changed restriction authority'; END IF; END $$;
RESET ROLE;
