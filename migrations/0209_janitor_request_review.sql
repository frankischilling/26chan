-- Atomic ordinary request review. Unsupported private-file/blacklist policies
-- remain explicit refusals rather than silently losing source effects.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request authority drifted before review'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA restrictions,content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
ALTER TABLE restrictions.templates ADD COLUMN request_blacklist_policy text
 CHECK(request_blacklist_policy IN('none','image','rejectimage'));
CREATE TABLE restrictions.request_totals (
 account_id bigint PRIMARY KEY CHECK(account_id>0),
 approved bigint NOT NULL DEFAULT 0 CHECK(approved>=0),
 denied bigint NOT NULL DEFAULT 0 CHECK(denied>=0)
);
CREATE TABLE restrictions.request_reviews (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 request_id bigint NOT NULL UNIQUE CHECK(request_id>0),
 account_id bigint NOT NULL CHECK(account_id>0),
 reviewer_id bigint NOT NULL CHECK(reviewer_id>0),
 board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$'),
 post_id bigint NOT NULL CHECK(post_id>0),
 requested_template bigint NOT NULL CHECK(requested_template BETWEEN 1 AND 2147483647),
 accepted_template bigint CHECK(accepted_template BETWEEN 1 AND 2147483647),
 approved boolean NOT NULL,
 warning_action boolean,
 ban_id bigint REFERENCES restrictions.bans(id),
 created_at timestamptz NOT NULL CHECK(isfinite(created_at)),
 CHECK((approved AND accepted_template IS NOT NULL AND warning_action IS NOT NULL AND ban_id IS NOT NULL)
  OR (NOT approved AND accepted_template IS NULL AND warning_action IS NULL AND ban_id IS NULL))
);
CREATE INDEX request_review_account_time ON restrictions.request_reviews(account_id,created_at DESC,id DESC);
CREATE INDEX request_review_expiry ON restrictions.request_reviews(created_at);
CREATE TABLE restrictions.request_ban_evidence (
 ban_id bigint PRIMARY KEY REFERENCES restrictions.bans(id) ON DELETE CASCADE,
 request_id bigint NOT NULL UNIQUE CHECK(request_id>0),
 payload jsonb NOT NULL CHECK(jsonb_typeof(payload)='object' AND octet_length(payload::text)<=33554432)
);
REVOKE ALL ON restrictions.request_totals,restrictions.request_reviews,
 restrictions.request_reviews_id_seq,restrictions.request_ban_evidence FROM PUBLIC;

CREATE FUNCTION restrictions.request_review_decision(p_seconds bigint,p_template_warning boolean,
 p_template_global boolean,p_json_only boolean,p_requested_warning boolean,p_amended boolean,
 p_days integer,p_global boolean,p_prior_local boolean)
RETURNS TABLE(seconds bigint,warning_action boolean,global boolean,duplicates text)
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF p_template_warning IS NULL OR p_template_global IS NULL OR p_json_only IS NULL
  OR p_requested_warning IS NULL OR p_amended IS NULL OR p_prior_local IS NULL
  OR (p_template_warning AND p_seconds IS DISTINCT FROM 0)
  OR (NOT p_template_warning AND p_seconds IS NOT NULL AND p_seconds NOT BETWEEN 1 AND 863913600)
  OR (p_amended AND p_days IS NOT NULL AND p_days NOT BETWEEN -1 AND 1024) THEN
  RAISE EXCEPTION 'Invalid request review decision' USING ERRCODE='23514'; END IF;
 warning_action:=(p_amended AND coalesce(p_days=0,false)) OR (NOT p_amended AND p_requested_warning);
 seconds:=CASE WHEN warning_action THEN 0
  WHEN p_amended AND p_days=-1 THEN NULL
  WHEN p_amended AND p_days IS NOT NULL THEN p_days::bigint*86400
  WHEN p_amended AND p_template_warning THEN 86400 ELSE p_seconds END;
 global:=CASE WHEN p_amended AND NOT warning_action THEN coalesce(p_global,p_template_global)
  ELSE p_template_global END;
 IF NOT p_amended AND NOT warning_action AND NOT global AND p_prior_local THEN global:=true; END IF;
 duplicates:=CASE WHEN p_json_only THEN 'none' WHEN warning_action THEN 'warnings' ELSE 'all' END;
 RETURN NEXT;
END $$;

CREATE FUNCTION content.review_janitor_request(p_session bytea,p_csrf bytea,p_idle integer,p_request bigint,
 p_approve boolean,p_template bigint,p_days integer,p_global boolean,p_public text,p_private text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp
 SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE account bigint; rank integer; target restrictions.requests%ROWTYPE; preset restrictions.templates%ROWTYPE;
 actor bytea; v_board text; decided_warning boolean; chosen bigint; amended boolean; decision record; stamp timestamptz;
 public_reason text; private_reason text; ban bigint; charge bigint; removed bigint; released bigint; ids bigint[];
BEGIN
 IF restrictions.invoker() NOT IN('board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Request review is unavailable' USING ERRCODE='42501'; END IF;
 IF p_request IS NULL OR p_request<=0 OR p_approve IS NULL
  OR (p_public IS NOT NULL AND octet_length(p_public)>16384)
  OR (p_private IS NOT NULL AND octet_length(p_private)>32768) THEN
  RAISE EXCEPTION 'Invalid request review' USING ERRCODE='23514'; END IF;
 account:=restrictions.request_authority(p_session,p_csrf,p_idle);
 SELECT restrictions.template_rank(a.role) INTO rank FROM staff_identity.accounts a WHERE a.id=account;
 IF rank<10 THEN RAISE EXCEPTION 'Moderator review is required' USING ERRCODE='P134A'; END IF;
 SELECT r.actor_hash,r.board INTO actor,v_board FROM restrictions.requests r WHERE r.id=p_request;
 IF NOT FOUND THEN
  IF p_approve THEN RAISE EXCEPTION 'Request is unavailable' USING ERRCODE='P134N'; END IF;
  RETURN jsonb_build_object('changed',false,'ban_id',NULL,'cleared',0); END IF;
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,v_board,true);
 PERFORM content.lock_posting_actor(actor,false);
 PERFORM restrictions.gate();
 PERFORM singleton FROM restrictions.request_capacity WHERE singleton FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Request capacity is unavailable' USING ERRCODE='P134R'; END IF;
 SELECT * INTO target FROM restrictions.requests r WHERE r.id=p_request AND r.actor_hash=actor AND r.board=v_board FOR UPDATE NOWAIT;
 IF NOT FOUND THEN
  IF p_approve THEN RAISE EXCEPTION 'Request is unavailable' USING ERRCODE='P134N'; END IF;
  RETURN jsonb_build_object('changed',false,'ban_id',NULL,'cleared',0); END IF;
 stamp:=date_trunc('second',clock_timestamp());
 ids:=ARRAY[p_request];
 IF p_approve THEN
  amended:=p_template IS NOT NULL;
  chosen:=coalesce(p_template,target.template_id);
  IF chosen NOT BETWEEN 1 AND 2147483647 OR chosen IN(130,131) THEN
   RAISE EXCEPTION 'Invalid request template' USING ERRCODE='23514'; END IF;
  SELECT * INTO preset FROM restrictions.templates t WHERE t.id=chosen AND t.enabled FOR SHARE NOWAIT;
  IF NOT FOUND OR restrictions.template_rank(preset.minimum_role)>rank THEN
   RAISE EXCEPTION 'Review template is unavailable' USING ERRCODE='P134A'; END IF;
  IF preset.request_save_policy IS NULL OR preset.request_save_policy='everything'
   OR preset.request_blacklist_policy IS DISTINCT FROM 'none' THEN
   RAISE EXCEPTION 'Review file effects are unavailable' USING ERRCODE='P0207'; END IF;
  SELECT * INTO decision FROM restrictions.request_review_decision(preset.seconds,preset.warning,preset.global,
   preset.request_save_policy='json_only',target.warning,amended,p_days,p_global,
   EXISTS(SELECT 1 FROM restrictions.bans b WHERE b.actor_hash=actor AND b.active AND NOT b.global AND b.ends_at>stamp));
  decided_warning:=decision.warning_action;
  IF decision.duplicates<>'none' THEN
   SELECT array_agg(q.id ORDER BY q.id) INTO ids FROM (
    SELECT r.id FROM restrictions.requests r WHERE r.id=p_request OR (r.actor_hash=actor
     AND r.template_id=chosen AND (decision.duplicates='all' OR r.warning)) ORDER BY r.id LIMIT 10001 FOR UPDATE
   ) q;
   IF cardinality(ids)>10000 THEN RAISE EXCEPTION 'Related requests exceed their bound' USING ERRCODE='P134C'; END IF;
   IF EXISTS(SELECT 1 FROM restrictions.requests r WHERE r.id=ANY(ids)
    AND NOT staff_identity.has_board_access(account,r.board)) THEN
    RAISE EXCEPTION 'Related request board access is unavailable' USING ERRCODE='P134A'; END IF;
  END IF;
  public_reason:=CASE WHEN amended AND coalesce(p_public,'')<>'' THEN p_public ELSE preset.public_reason END;
  private_reason:=preset.private_reason||CASE WHEN preset.private_reason='' THEN '' ELSE ', ' END
   ||'ban requested by account '||target.account_id;
  IF amended AND coalesce(p_private,'')<>'' THEN private_reason:=private_reason||'. '||p_private; END IF;
  IF octet_length(public_reason) NOT BETWEEN 1 AND 16384 OR octet_length(private_reason)>32768 THEN
   RAISE EXCEPTION 'Review reason exceeds its bound' USING ERRCODE='23514'; END IF;
  charge:=512+octet_length(public_reason)::bigint+octet_length(private_reason)::bigint+64
   +CASE WHEN target.normalized_upload_md5 IS NULL THEN 0 ELSE 16 END+128+octet_length(target.payload::text)::bigint;
  IF EXISTS(SELECT 1 FROM restrictions.capacity c WHERE c.singleton AND (c.used_bytes>c.byte_limit-charge
   OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans LIMIT 1000001) bounded)>=c.row_limit)) THEN
   RAISE EXCEPTION 'Restriction capacity is exhausted' USING ERRCODE='P134C'; END IF;
  INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,starts_at,ends_at,
   public_reason,private_reason,account_id,charged_bytes,template_id,upload_observation,normalized_upload_md5)
  VALUES(target.board,target.post_id,actor,target.automatic_identity,decision.global,coalesce(decision.seconds=0,false),stamp,
   CASE WHEN decision.seconds IS NULL THEN NULL ELSE stamp+make_interval(secs=>decision.seconds::double precision) END,
   public_reason,private_reason,account,charge,chosen,target.upload_observation,target.normalized_upload_md5) RETURNING id INTO ban;
  INSERT INTO restrictions.request_ban_evidence(ban_id,request_id,payload) VALUES(ban,p_request,target.payload);
  UPDATE restrictions.capacity SET used_bytes=used_bytes+charge WHERE singleton;
  INSERT INTO restrictions.audit(ban_id,action,account_id)
   VALUES(ban,CASE WHEN decision.seconds=0 THEN 'warning' ELSE 'ban' END,account);
 END IF;
 -- Six calendar months of detailed outcomes, separate from cumulative totals.
 DELETE FROM restrictions.request_reviews WHERE created_at<stamp-interval '6 months';
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.request_reviews LIMIT 100001) bounded)>=100000
  OR (NOT EXISTS(SELECT 1 FROM restrictions.request_totals WHERE account_id=target.account_id)
   AND (SELECT count(*) FROM (SELECT 1 FROM restrictions.request_totals LIMIT 10001) bounded)>=10000) THEN
  RAISE EXCEPTION 'Request statistics capacity is exhausted' USING ERRCODE='P134C'; END IF;
 INSERT INTO restrictions.request_totals(account_id,approved,denied)
  VALUES(target.account_id,CASE WHEN p_approve THEN 1 ELSE 0 END,CASE WHEN p_approve THEN 0 ELSE 1 END)
 ON CONFLICT(account_id) DO UPDATE SET approved=request_totals.approved+EXCLUDED.approved,
  denied=request_totals.denied+EXCLUDED.denied;
 INSERT INTO restrictions.request_reviews(request_id,account_id,reviewer_id,board,post_id,requested_template,
  accepted_template,approved,warning_action,ban_id,created_at)
 VALUES(p_request,target.account_id,account,target.board,target.post_id,target.template_id,
  CASE WHEN p_approve THEN chosen END,p_approve,decided_warning,ban,stamp);
 WITH gone AS (DELETE FROM restrictions.requests r WHERE r.id=ANY(ids) RETURNING r.charged_bytes)
 SELECT count(*),coalesce(sum(charged_bytes),0) INTO removed,released FROM gone;
 IF removed<>cardinality(ids) THEN RAISE EXCEPTION 'Reviewed request set changed' USING ERRCODE='P134R'; END IF;
 UPDATE restrictions.request_capacity SET used_bytes=used_bytes-released WHERE singleton;
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,target.board,true);
 RETURN jsonb_build_object('changed',true,'ban_id',ban,'cleared',removed);
END $$;
REVOKE ALL ON FUNCTION restrictions.request_review_decision(bigint,boolean,boolean,boolean,boolean,boolean,integer,boolean,boolean),
 content.review_janitor_request(bytea,bytea,integer,bigint,boolean,bigint,integer,boolean,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.review_janitor_request(bytea,bytea,integer,bigint,boolean,bigint,integer,boolean,text,text) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,content FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request review changed restriction authority'; END IF; END $$;
RESET ROLE;
