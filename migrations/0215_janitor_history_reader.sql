-- Scoped retained ban history reached from an authorized pending request.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request authority drifted before history reader'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA staff_identity TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE FUNCTION staff_identity.janitor_request_history(p_session bytea,p_csrf bytea,p_idle integer,p_request bigint,p_before bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; rank integer; target restrictions.requests%ROWTYPE; rows jsonb; next_id bigint; amount integer;
BEGIN
 IF restrictions.invoker() NOT IN('board_auth','board_migrator') THEN
  RAISE EXCEPTION 'Request history is unavailable' USING ERRCODE='42501'; END IF;
 IF p_request IS NULL OR p_request<=0 OR p_before IS NULL OR p_before<0 THEN
  RAISE EXCEPTION 'Invalid history cursor' USING ERRCODE='23514'; END IF;
 account:=restrictions.message_authority(p_session,p_csrf,p_idle);
 SELECT restrictions.template_rank(a.role) INTO rank FROM staff_identity.accounts a WHERE a.id=account;
 IF rank<10 THEN RAISE EXCEPTION 'Request history is unavailable' USING ERRCODE='P134A'; END IF;
 PERFORM restrictions.gate(false);
 SELECT * INTO target FROM restrictions.requests WHERE id=p_request FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Request is unavailable' USING ERRCODE='P134N'; END IF;
 IF NOT staff_identity.has_board_access(account,target.board) THEN
  RAISE EXCEPTION 'Request history scope is unavailable' USING ERRCODE='P134A'; END IF;
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans WHERE actor_hash=target.actor_hash LIMIT 10001) bounded)>10000 THEN
  RAISE EXCEPTION 'Request history exceeds its bound' USING ERRCODE='P134C'; END IF;
 WITH selected AS (
  SELECT b.* FROM restrictions.bans b WHERE b.actor_hash=target.actor_hash
   AND (p_before=0 OR b.id<p_before) AND staff_identity.has_board_access(account,b.board)
  ORDER BY b.id DESC LIMIT 26
 ), numbered AS (SELECT *,row_number() OVER(ORDER BY id DESC) AS number FROM selected)
 SELECT count(*),min(id) FILTER(WHERE number<=25),coalesce(jsonb_agg(jsonb_build_object(
  'id',id,'board',board,'post_id',post_id,'global',global,'warning',warning,
  'starts_at',floor(extract(epoch FROM starts_at))::bigint,
  'ends_at',floor(extract(epoch FROM ends_at))::bigint,
  'public_reason',public_reason,'private_reason',private_reason,
  'active',active AND (warning OR ends_at IS NULL OR ends_at>statement_timestamp()),
  'retirement',retirement,'retired_at',floor(extract(epoch FROM retired_at))::bigint)
  ORDER BY id DESC) FILTER(WHERE number<=25),'[]') INTO amount,next_id,rows FROM numbered;
 PERFORM restrictions.message_authority(p_session,p_csrf,p_idle);
 RETURN jsonb_build_object('request_id',p_request,'board',target.board,'rows',rows,
  'next_before',CASE WHEN amount=26 THEN next_id ELSE 0 END);
END $$;
REVOKE ALL ON FUNCTION staff_identity.janitor_request_history(bytea,bytea,integer,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.janitor_request_history(bytea,bytea,integer,bigint,bigint) TO board_auth;
RESET ROLE;
REVOKE CREATE ON SCHEMA staff_identity FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request history authority failed'; END IF; END $$;
RESET ROLE;
