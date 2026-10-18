-- A bounded, session-scoped form reader. Request target identity and unlock
-- weights stay private; the browser receives no authority assertion to replay.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request authority drifted before form reader'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA staff_identity TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE FUNCTION staff_identity.janitor_request_form(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_post bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; rank integer; actor bytea; choices jsonb; related bigint; subject text;
BEGIN
 IF restrictions.invoker() NOT IN('board_auth','board_migrator') THEN
  RAISE EXCEPTION 'Request form is unavailable' USING ERRCODE='42501'; END IF;
 IF p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_board='j' OR p_post IS NULL OR p_post<=0 THEN
  RAISE EXCEPTION 'Invalid request target' USING ERRCODE='23514'; END IF;
 account:=restrictions.message_authority(p_session,p_csrf,p_idle);
 SELECT restrictions.template_rank(a.role) INTO rank FROM staff_identity.accounts a WHERE a.id=account;
 PERFORM restrictions.gate(false);
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Request target is unavailable' USING ERRCODE='P134N'; END IF;
 IF NOT staff_identity.has_board_access(account,p_board)
  AND NOT post_secrets.janitor_request_unlocked(p_board,p_post) THEN
  RAISE EXCEPTION 'Request board access is unavailable' USING ERRCODE='P134A'; END IF;
 SELECT h.actor_hash INTO actor FROM content.posts p
  JOIN content.threads t ON t.board=p.board AND t.id=p.thread_id
  JOIN post_secrets.posting_history h ON h.board=p.board AND h.post_id=p.id
  WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted AND NOT t.deleted AND t.archived_at IS NULL;
 IF NOT FOUND THEN RAISE EXCEPTION 'Request target is unavailable' USING ERRCODE='P134N'; END IF;
 IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request snapshot authority is unavailable' USING ERRCODE='P134R'; END IF;
 SELECT left(s.payload->>'subject',120) INTO subject FROM content.report_snapshot_source s
  WHERE s.board=p_board AND s.post_id=p_post;
 IF NOT FOUND THEN RAISE EXCEPTION 'Request target is unavailable' USING ERRCODE='P134N'; END IF;
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.templates LIMIT 129) bounded)>128 THEN
  RAISE EXCEPTION 'Request template inventory exceeds its bound' USING ERRCODE='P134C'; END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',t.id,'name',t.name,'warning',t.warning,
  'can_warn',t.request_can_warn,'requires_file',t.requires_file,
  'available',coalesce(t.request_save_policy IN('none','json_only'),false)) ORDER BY t.id),'[]')
 INTO choices FROM restrictions.templates t WHERE t.enabled AND restrictions.template_rank(t.minimum_role)<=rank;
 SELECT count(*) INTO related FROM (SELECT 1 FROM restrictions.requests r WHERE r.actor_hash=actor LIMIT 10001) bounded;
 IF related>10000 THEN RAISE EXCEPTION 'Related request inventory exceeds its bound' USING ERRCODE='P134C'; END IF;
 PERFORM restrictions.message_authority(p_session,p_csrf,p_idle);
 RETURN jsonb_build_object('subject',subject,'templates',choices,'related_count',related);
END $$;
REVOKE ALL ON FUNCTION staff_identity.janitor_request_form(bytea,bytea,integer,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.janitor_request_form(bytea,bytea,integer,text,bigint) TO board_auth;
RESET ROLE;
REVOKE CREATE ON SCHEMA staff_identity FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request form changed restriction authority'; END IF; END $$;
RESET ROLE;
