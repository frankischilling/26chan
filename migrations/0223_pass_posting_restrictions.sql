-- Bind active Pass identity to posting restrictions without exposing it to runtimes.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass restriction matching'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass restriction matching'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass restriction matching'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass restriction matching'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass restriction matching'; END IF; END $$;
GRANT USAGE ON SCHEMA verification_private TO board_restriction_owner;
GRANT CREATE ON SCHEMA verification_private,content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
GRANT SELECT ON verification_private.pass_posts TO board_restriction_owner;
GRANT EXECUTE ON FUNCTION content.public_pass_ready() TO board_restriction_owner;
CREATE FUNCTION verification_private.pass_posting_identity(p_board text,p_target bigint,p_anonymous bytea) RETURNS bytea
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE raw text; proof jsonb; identity bytea;
BEGIN
 PERFORM verification_private.pass_checked_post_bypass(p_board,p_target);
 raw:=current_setting('board.challenge_context',true);
 IF raw IS NULL OR raw='' THEN RETURN NULL; END IF;
 -- The shared checked helper has already validated shape and retained all
 -- receipt, session and entitlement locks. No second untrusted flag is read.
 proof:=raw::jsonb;
 IF NOT EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=decode(proof->>'receipt','hex')) THEN RETURN NULL; END IF;
 IF p_anonymous IS NULL OR decode(proof->>'session','hex')<>p_anonymous THEN
 RAISE EXCEPTION 'Membership anonymous identity differs' USING ERRCODE='P0166'; END IF;
 SELECT e.token_hash INTO STRICT identity FROM verification_private.pass_receipts r
 JOIN verification_private.pass_sessions s ON s.token_hash=r.pass_session
 JOIN verification_private.pass_entitlements e ON e.id=s.entitlement_id
 WHERE r.token_hash=decode(proof->>'receipt','hex');
 RETURN identity;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_posting_identity(text,bigint,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION verification_private.pass_posting_identity(text,bigint,bytea) TO board_restriction_owner;
CREATE FUNCTION verification_private.pass_inserted_identity(p_post bigint,p_anonymous bytea) RETURNS bytea
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 PERFORM verification_private.pass_gate();
 RETURN (SELECT p.identity_hash FROM verification_private.pass_posts p
 JOIN verification_private.pass_post_handoffs h ON h.post_id=p.post_id
 WHERE p.post_id=p_post AND h.session_hash=p_anonymous AND h.registration_xid=pg_current_xact_id());
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_inserted_identity(bigint,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION verification_private.pass_inserted_identity(bigint,bytea) TO board_restriction_owner;
CREATE FUNCTION content.pass_network_bypass(p_board text,p_target bigint,p_anonymous bytea) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_public' THEN
 RAISE EXCEPTION 'Public network membership unavailable' USING ERRCODE='42501'; END IF;
 RETURN verification_private.pass_posting_identity(p_board,p_target,p_anonymous) IS NOT NULL;
END $$;
REVOKE ALL ON FUNCTION content.pass_network_bypass(text,bigint,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_network_bypass(text,bigint,bytea) TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private,content FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA restrictions,content,staff_identity TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
ALTER TABLE restrictions.bans ADD COLUMN pass_identity_hash bytea CHECK(pass_identity_hash IS NULL OR octet_length(pass_identity_hash)=32);
CREATE INDEX restriction_pass_active ON restrictions.bans(pass_identity_hash,id) WHERE active AND pass_identity_hash IS NOT NULL;
ALTER TABLE restrictions.requests ADD COLUMN pass_identity_hash bytea CHECK(pass_identity_hash IS NULL OR octet_length(pass_identity_hash)=32);
CREATE FUNCTION restrictions.capture_request_pass_identity() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'restrictions' OR TG_TABLE_NAME<>'requests' OR TG_LEVEL<>'ROW'
 OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 OR TG_OP NOT IN('INSERT','UPDATE') THEN
 RAISE EXCEPTION 'Invalid request membership capture' USING ERRCODE='23514'; END IF;
 IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Membership authority unavailable' USING ERRCODE='P134R'; END IF;
 IF TG_OP='INSERT' THEN
  NEW.pass_identity_hash:=(SELECT p.identity_hash FROM verification_private.pass_posts p WHERE p.post_id=NEW.post_id);
 ELSIF NEW.pass_identity_hash IS DISTINCT FROM OLD.pass_identity_hash THEN
  RAISE EXCEPTION 'Request membership identity is immutable' USING ERRCODE='42501';
 END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION restrictions.capture_request_pass_identity() FROM PUBLIC;
CREATE TRIGGER capture_request_pass_identity BEFORE INSERT OR UPDATE ON restrictions.requests FOR EACH ROW EXECUTE FUNCTION restrictions.capture_request_pass_identity();
CREATE FUNCTION restrictions.capture_pass_identity() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'restrictions' OR TG_TABLE_NAME<>'bans' OR TG_LEVEL<>'ROW'
 OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 OR TG_OP NOT IN('INSERT','UPDATE') THEN
 RAISE EXCEPTION 'Invalid membership restriction capture' USING ERRCODE='23514'; END IF;
 IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Membership authority unavailable' USING ERRCODE='P134R'; END IF;
 -- Direct bans derive live post identity; reviewed requests supply their
 -- already-captured private identity after request creation erased the post.
 IF TG_OP='INSERT' AND NEW.origin='staff' AND NEW.pass_identity_hash IS NULL THEN
  NEW.pass_identity_hash:=(SELECT p.identity_hash FROM verification_private.pass_posts p WHERE p.post_id=NEW.post_id);
 ELSIF TG_OP='UPDATE' AND NEW.pass_identity_hash IS DISTINCT FROM OLD.pass_identity_hash THEN
  RAISE EXCEPTION 'Restriction membership identity is immutable' USING ERRCODE='42501';
 END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION restrictions.capture_pass_identity() FROM PUBLIC;
CREATE TRIGGER capture_pass_identity BEFORE INSERT OR UPDATE ON restrictions.bans FOR EACH ROW EXECUTE FUNCTION restrictions.capture_pass_identity();
CREATE FUNCTION restrictions.posting_decision_qualified(p_actor bytea,p_token bytea,p_board text,p_request bigint,p_membership bytea)
RETURNS TABLE(disposition text,expired bigint[]) LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_automatic uuid; v_new boolean; v_verified boolean:=false; v_rows jsonb; v_count integer;
BEGIN
 IF (p_membership IS NOT NULL AND octet_length(p_membership)<>32) OR p_actor IS NULL OR octet_length(p_actor)<>32 OR (p_token IS NOT NULL AND octet_length(p_token)<>32)
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_request IS NULL OR p_request<=0 THEN
  RAISE EXCEPTION 'Invalid restriction context' USING ERRCODE='23514';
 END IF;
 -- Empty state has no session/time decision. Historical controlled fixtures
 -- and operator imports do not manufacture a current anonymous capability.
 IF NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE active) THEN
  RETURN QUERY SELECT 'allowed'::text,ARRAY[]::bigint[]; RETURN;
 END IF;
 SELECT a.automatic_identity,a.source_new INTO v_automatic,v_new
    FROM post_secrets.lookup_automatic_identity(p_token,p_request) a;
 IF v_new THEN v_automatic:=NULL;
 ELSE
  SELECT coalesce(s.verified_level>0,false) INTO v_verified FROM post_secrets.anonymous_sessions s
    WHERE s.token_hash=p_token AND s.expires_at>extract(epoch FROM clock_timestamp())::bigint;
  v_verified:=coalesce(v_verified,false);
 END IF;
 -- Source queries password first, then address. A row matching both fields
 -- is visited twice, including duplicate expired IDs, just like the source.
 WITH matches AS (
  SELECT b.*,0 AS position,'password'::text AS key FROM restrictions.bans b
    WHERE b.active AND v_automatic IS NOT NULL AND b.automatic_identity=v_automatic
  UNION ALL
  SELECT b.*,1 AS position,'4pass_id'::text AS key FROM restrictions.bans b
    WHERE b.active AND p_membership IS NOT NULL AND b.pass_identity_hash=p_membership
  UNION ALL
  SELECT b.*,2 AS position,'host'::text AS key FROM restrictions.bans b WHERE b.active AND b.actor_hash=p_actor AND p_membership IS NULL
 ), bounded AS (SELECT * FROM matches ORDER BY position,id LIMIT 1001)
 SELECT count(*),coalesce(jsonb_agg(jsonb_build_object('no',id,'key',key,'board',board,'global',global,
    'starts_on',floor(extract(epoch FROM starts_at))::bigint,
    'ends_on',CASE WHEN ends_at IS NULL THEN 0 ELSE floor(extract(epoch FROM ends_at))::bigint END,
    'template_id',coalesce(template_id,0),'admin',CASE WHEN origin='staff' THEN 'Staff' ELSE 'Auto-ban' END,
    'reason',CASE WHEN origin='staff' THEN '' ELSE public_reason||'<>'||private_reason END) ORDER BY position,id),'[]'::jsonb)
 INTO v_count,v_rows FROM bounded;
 IF v_count>1000 THEN RAISE EXCEPTION 'Restriction context exceeds its bound' USING ERRCODE='P134C'; END IF;
 -- Membership is supplied only by locked receipts or the current insert handoff.
 RETURN QUERY SELECT d.disposition,d.expired FROM restrictions.source_decision(v_rows,p_board,p_request,p_membership IS NOT NULL,p_membership IS NOT NULL,v_verified) d;
END $$;

REVOKE ALL ON FUNCTION restrictions.posting_decision_qualified(bytea,bytea,text,bigint,bytea) FROM PUBLIC;
CREATE OR REPLACE FUNCTION restrictions.posting_decision(p_actor bytea,p_token bytea,p_board text,p_request bigint)
RETURNS TABLE(disposition text,expired bigint[]) LANGUAGE sql SET search_path=pg_catalog,pg_temp AS $$
 SELECT * FROM restrictions.posting_decision_qualified(p_actor,p_token,p_board,p_request,NULL)
$$;
CREATE FUNCTION restrictions.evaluate_qualified(p_actor bytea,p_token bytea,p_board text,p_request bigint,p_membership bytea)
RETURNS text LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_state text; v_expired bigint[]; v_id bigint;
BEGIN
 IF restrictions.invoker() NOT IN('board_public','board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Restriction evaluation is unavailable' USING ERRCODE='42501';
 END IF;
 PERFORM restrictions.gate(false);
 SELECT d.disposition,d.expired INTO v_state,v_expired FROM restrictions.posting_decision_qualified(p_actor,p_token,p_board,p_request,p_membership) d;
 IF cardinality(v_expired)>0 THEN
  PERFORM restrictions.gate(true);
  FOR v_id IN UPDATE restrictions.bans SET active=false,retired_at=clock_timestamp(),retirement='expiration'
     WHERE id=ANY(v_expired) AND active AND NOT warning AND floor(extract(epoch FROM ends_at))<=p_request
     RETURNING id LOOP
   INSERT INTO restrictions.audit(ban_id,action) VALUES(v_id,'expiration');
  END LOOP;
 END IF;
 RETURN v_state;
END $$;

REVOKE ALL ON FUNCTION restrictions.evaluate_qualified(bytea,bytea,text,bigint,bytea) FROM PUBLIC;
CREATE FUNCTION restrictions.record_file_ban_with_membership(
 p_actor bytea,p_token bytea,p_board text,p_rule bigint,p_digest bytea,
 p_days integer,p_dmca boolean,p_public text,p_private text,p_timezone text,p_membership bytea
) RETURNS bigint LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' AS $$
DECLARE v_now timestamptz; v_end timestamptz; v_identity uuid; v_id bigint; v_charge bigint;
 c restrictions.capacity%ROWTYPE;
BEGIN
 IF (p_membership IS NOT NULL AND octet_length(p_membership)<>32) OR p_actor IS NULL OR octet_length(p_actor)<>32 OR (p_token IS NOT NULL AND octet_length(p_token)<>32)
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_rule IS NULL OR p_rule<=0
    OR p_digest IS NULL OR octet_length(p_digest)<>16 OR p_dmca IS NULL
    OR (p_dmca AND p_days IS DISTINCT FROM 3)
    OR p_public IS NULL OR octet_length(p_public) NOT BETWEEN 1 AND 16384
    OR p_private IS NULL OR octet_length(p_private)>32768 THEN
  RAISE EXCEPTION 'Invalid private file restriction.' USING ERRCODE='23514'; END IF;
 -- Invoke from a fresh effect transaction after posting rollback, before any
 -- board/media locks. A caller must never upgrade its posting gate in place.
 PERFORM content.lock_posting_actor(p_actor,false);
 PERFORM restrictions.gate(true);
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR SHARE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'File restriction board is unavailable.' USING ERRCODE='P134N'; END IF;
 v_now:=date_trunc('second',clock_timestamp());
 v_end:=restrictions.file_expiry(v_now,p_days,p_timezone);
 SELECT a.automatic_identity INTO v_identity
  FROM post_secrets.lookup_automatic_identity(p_token,floor(extract(epoch FROM v_now))::bigint) a;
 -- Existing trusted equality is retained even for a creation-second/idle-reset
 -- session. Unknown, expired and not-yet-persisted sessions yield no identity.
 SELECT * INTO STRICT c FROM restrictions.capacity WHERE singleton;
 v_charge:=512+octet_length(p_public)::bigint+octet_length(p_private)::bigint;
 IF c.used_bytes>c.byte_limit-v_charge
    OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans LIMIT c.row_limit) bounded)>=c.row_limit THEN
  RAISE EXCEPTION 'Restriction capacity is exhausted.' USING ERRCODE='P134C'; END IF;
 INSERT INTO restrictions.bans(board,actor_hash,automatic_identity,global,warning,starts_at,ends_at,
  public_reason,private_reason,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash)
 VALUES(p_board,p_actor,v_identity,true,v_end IS NOT NULL AND v_end<=v_now,v_now,v_end,
  p_public,p_private,v_charge,CASE WHEN p_dmca THEN 'dmca' ELSE 'file' END,p_rule,p_digest,p_membership)
 RETURNING id INTO v_id;
 UPDATE restrictions.capacity SET used_bytes=used_bytes+v_charge WHERE singleton;
 INSERT INTO restrictions.audit(ban_id,action)
 VALUES(v_id,CASE WHEN v_end IS NOT NULL AND v_end<=v_now THEN 'warning' ELSE 'ban' END);
 RETURN v_id;
END $$;
REVOKE ALL ON FUNCTION restrictions.record_file_ban_with_membership(bytea,bytea,text,bigint,bytea,integer,boolean,text,text,text,bytea) FROM PUBLIC;
CREATE OR REPLACE FUNCTION content.finalize_file_filter(
 p_actor bytea,p_peer inet,p_token bytea,p_board text,p_parent bigint,p_job text,p_capability text,
 p_revision bigint,p_rule bigint,p_digest bytea,p_expected jsonb,
 p_session bytea,p_csrf bytea,p_idle integer,p_cookie jsonb
) RETURNS TABLE(response text,diagnostic text,quiet_post bigint,cookie_refreshed boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE m record; again record; r record; staff_role text; current_role_name text; authorized boolean:=false;
 v_repeat boolean:=false; v_exempt boolean; v_zone text; private_reason text; public_reason text;
 membership bytea; v_response text; v_diagnostic text; v_quiet bigint; v_cookie boolean:=false;
BEGIN
 IF session_user NOT IN('board_public','board_staff')
    OR current_setting('transaction_isolation')<>'read committed'
    OR p_actor IS NULL OR octet_length(p_actor)<>32
    OR p_peer IS NULL OR masklen(p_peer)<>(CASE family(p_peer) WHEN 4 THEN 32 ELSE 128 END)
    OR (p_token IS NOT NULL AND octet_length(p_token)<>32)
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_parent IS NULL OR p_parent<0
    OR p_rule IS NULL OR p_rule<1 OR p_digest IS NULL OR octet_length(p_digest)<>16
    OR p_expected IS NULL OR jsonb_typeof(p_expected)<>'object' OR octet_length(p_expected::text)>4096
    OR (p_cookie IS NOT NULL AND (jsonb_typeof(p_cookie)<>'object' OR octet_length(p_cookie::text)>1024)) THEN
  RAISE EXCEPTION 'Invalid file refusal context.' USING ERRCODE='23514'; END IF;
 IF session_user='board_public' AND (p_session IS NOT NULL OR p_csrf IS NOT NULL OR p_idle IS NOT NULL)
    OR session_user='board_staff' AND (p_token IS NOT NULL OR p_cookie IS NOT NULL) THEN
  RAISE EXCEPTION 'Invalid file refusal authority.' USING ERRCODE='28000'; END IF;
 PERFORM content.lock_posting_actor(p_actor,false);
 PERFORM restrictions.gate(true);
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'File refusal authority catalog is unavailable.' USING ERRCODE='55000'; END IF;
 PERFORM slug FROM content.boards WHERE slug=p_board AND NOT staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 IF session_user='board_public' THEN membership:=verification_private.pass_posting_identity(p_board,p_parent,p_token); END IF;
 -- Resolve against the original snapshot first to acquire the policy lock
 -- before media locks. It will be resolved again from actual upload metadata.
 SELECT * INTO r FROM admission.resolve_file_effect_rule(p_board,p_revision,
  decode(p_expected->>'normalized','hex'),decode(p_expected->>'original','hex'),(p_expected->>'eligible')::boolean);
 IF NOT FOUND OR r.rule_id<>p_rule OR r.digest<>p_digest THEN
  RAISE EXCEPTION 'File refusal policy changed.' USING ERRCODE='55000'; END IF;
 IF session_user='board_staff' THEN
  SELECT a.role INTO staff_role FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id
   WHERE s.token_hash=p_session;
  IF NOT FOUND THEN RAISE EXCEPTION 'Staff refusal authority changed.' USING ERRCODE='28000'; END IF;
  authorized:=staff_role IN('moderator','manager','admin');
 END IF;
 SELECT * INTO STRICT m FROM content.file_effect_upload_metadata(p_job,p_capability,p_board,p_parent,authorized);
 IF jsonb_build_object('filename',m.filename,'normalized',m.normalized_md5,
      'original',encode(m.original_md5,'hex'),'eligible',m.original_eligible) IS DISTINCT FROM p_expected THEN
  RAISE EXCEPTION 'File refusal upload changed.' USING ERRCODE='55000'; END IF;
 SELECT * INTO r FROM admission.resolve_file_effect_rule(p_board,p_revision,
  decode(m.normalized_md5,'hex'),m.original_md5,m.original_eligible);
 IF NOT FOUND OR r.rule_id<>p_rule OR r.digest<>p_digest THEN
  RAISE EXCEPTION 'File refusal match changed.' USING ERRCODE='55000'; END IF;
 IF session_user='board_staff' THEN
  PERFORM content.revalidate_staff_refusal(p_session,p_csrf,p_idle,p_board);
  SELECT a.role INTO current_role_name FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id
   WHERE s.token_hash=p_session;
  IF current_role_name IS DISTINCT FROM staff_role THEN
   RAISE EXCEPTION 'Staff refusal limits changed.' USING ERRCODE='28000'; END IF;
 END IF;
 private_reason:=restrictions.file_audit_reason(r.digest,m.audit_filename);
 -- Never disclose the private audit reason when the source's optional
 -- public reason is empty (including PHP's false string "0").
 public_reason:=CASE WHEN r.public_reason IN('','0') THEN 'Posting a blocked file.' ELSE r.public_reason END;
 IF r.mode=2 THEN
  v_repeat:=admission.file_effect_dmca(p_actor);
  v_response:=CASE WHEN v_repeat THEN 'banned' ELSE 'dmca' END;
  IF v_repeat THEN
   private_reason:='DMCA complaint from '||r.description||' (blacklist ID: '||r.rule_id||')';
   public_reason:='Repeatedly uploading image(s) subject to a copyright infringement claim.';
  END IF;
 ELSIF r.mode=0 AND p_board='test' THEN
  v_response:='diagnostic';
  -- Plain text for the normal escaping response renderer; stored audit text
  -- remains source-escaped and is never used as trusted response markup.
  v_diagnostic:='Blacklisted md5 - '||encode(r.digest,'hex')||' - Filename: '||m.audit_filename;
 ELSE
  v_response:=CASE WHEN r.quiet THEN 'quiet' ELSE 'failed' END;
 END IF;
 IF r.mode=1 OR (r.mode=2 AND v_repeat) THEN
  v_exempt:=admission.file_effect_exempt(p_peer);
  IF NOT v_exempt THEN
   SELECT timezone INTO v_zone FROM restrictions.file_clock WHERE singleton FOR SHARE;
   IF NOT FOUND OR v_zone IS NULL THEN
    RAISE EXCEPTION 'File ban timezone is not configured.' USING ERRCODE='55000'; END IF;
   PERFORM restrictions.record_file_ban_with_membership(p_actor,p_token,p_board,r.rule_id,r.digest,
    CASE WHEN r.mode=2 THEN 3 ELSE r.ban_days END,r.mode=2,public_reason,private_reason,v_zone,membership);
  END IF;
 END IF;
 IF v_response='quiet' THEN v_quiet:=restrictions.file_quiet_post(p_board,p_parent); END IF;
 IF p_cookie IS NOT NULL THEN
  BEGIN
   PERFORM post_secrets.resume_anonymous_refusal(p_token,decode(p_cookie->>'network','hex'),
    decode(p_cookie->>'address','hex'),decode(p_cookie->>'environment','hex'),
    (p_cookie->>'minted')::boolean,(p_cookie->>'now')::bigint);
   v_cookie:=true;
  EXCEPTION WHEN OTHERS THEN
   -- A failed optional refresh does not erase a definite file refusal.
   v_cookie:=false;
  END;
 END IF;
 -- Both time-bound capabilities are checked again after any effect/cookie wait.
 SELECT * INTO STRICT again FROM content.file_effect_upload_metadata(p_job,p_capability,p_board,p_parent,authorized);
 IF to_jsonb(again) IS DISTINCT FROM to_jsonb(m) THEN
  RAISE EXCEPTION 'File refusal upload changed before commit.' USING ERRCODE='55000'; END IF;
 IF session_user='board_staff' THEN
  PERFORM content.revalidate_staff_refusal(p_session,p_csrf,p_idle,p_board);
 END IF;
 IF session_user='board_public' AND verification_private.pass_posting_identity(p_board,p_parent,p_token) IS DISTINCT FROM membership THEN
 RAISE EXCEPTION 'Membership refusal authority changed' USING ERRCODE='P0166'; END IF;
 RETURN QUERY SELECT v_response,v_diagnostic,v_quiet,v_cookie;
END $$;
REVOKE ALL ON FUNCTION content.finalize_file_filter(bytea,inet,bytea,text,bigint,text,text,bigint,bigint,bytea,jsonb,bytea,bytea,integer,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.finalize_file_filter(bytea,inet,bytea,text,bigint,text,text,bigint,bigint,bytea,jsonb,bytea,bytea,integer,jsonb) TO board_public,board_staff;
CREATE OR REPLACE FUNCTION content.review_janitor_request(p_session bytea,p_csrf bytea,p_idle integer,p_request bigint,
 p_approve boolean,p_template bigint,p_days integer,p_global boolean,p_public text,p_private text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp
 SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE account bigint; rank integer; target restrictions.requests%ROWTYPE; preset restrictions.templates%ROWTYPE;
 actor bytea; v_board text; decided_warning boolean; chosen bigint; amended boolean; decision record; stamp timestamptz;
 public_reason text; private_reason text; ban bigint; charge bigint; removed bigint; released bigint; ids bigint[]; retained_id bigint; retained record;
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
  IF preset.request_save_policy IS NULL
   OR preset.request_blacklist_policy IS NULL OR preset.request_blacklist_policy NOT IN('none','image','rejectimage') THEN
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
   public_reason,private_reason,account_id,charged_bytes,template_id,upload_observation,normalized_upload_md5,pass_identity_hash)
  VALUES(target.board,target.post_id,actor,target.automatic_identity,decision.global,coalesce(decision.seconds=0,false),stamp,
   CASE WHEN decision.seconds IS NULL THEN NULL ELSE stamp+make_interval(secs=>decision.seconds::double precision) END,
   public_reason,private_reason,account,charge,chosen,target.upload_observation,target.normalized_upload_md5,target.pass_identity_hash) RETURNING id INTO ban;
  INSERT INTO restrictions.request_ban_evidence(ban_id,request_id,payload) VALUES(ban,p_request,target.payload);
  UPDATE restrictions.capacity SET used_bytes=used_bytes+charge WHERE singleton;
  INSERT INTO restrictions.audit(ban_id,action,account_id)
   VALUES(ban,CASE WHEN decision.seconds=0 THEN 'warning' ELSE 'ban' END,account);
  IF amended AND preset.request_save_policy='json_only' THEN DELETE FROM restrictions.request_media WHERE request_id=p_request; END IF;
  PERFORM restrictions.issue_request_file_rule(target,preset,account,ban,decided_warning);
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
 IF p_approve AND NOT (amended AND preset.request_save_policy='json_only') THEN
  -- Keep thumbnails of the reviewed request and its cleared duplicates. Full
  -- representations leave custody before the thumbnail charge is admitted.
  FOR retained_id IN SELECT m.request_id FROM restrictions.request_media m WHERE m.request_id=ANY(ids) ORDER BY m.request_id LOOP
   DELETE FROM restrictions.request_media WHERE request_id=retained_id
    RETURNING thumbnail,thumbnail_format,thumbnail_width,thumbnail_height INTO retained;
   INSERT INTO restrictions.request_ban_thumbnails(source_request_id,ban_id,board,post_id,thumbnail,thumbnail_format,width,height)
    SELECT retained_id,ban,r.board,r.post_id,retained.thumbnail,retained.thumbnail_format,retained.thumbnail_width,retained.thumbnail_height
    FROM restrictions.requests r WHERE r.id=retained_id;
  END LOOP;
 END IF;
 WITH gone AS (DELETE FROM restrictions.requests r WHERE r.id=ANY(ids) RETURNING r.charged_bytes)
 SELECT count(*),coalesce(sum(charged_bytes),0) INTO removed,released FROM gone;
 IF removed<>cardinality(ids) THEN RAISE EXCEPTION 'Reviewed request set changed' USING ERRCODE='P134R'; END IF;
 UPDATE restrictions.request_capacity SET used_bytes=used_bytes-released WHERE singleton;
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,target.board,true);
 RETURN jsonb_build_object('changed',true,'ban_id',ban,'cleared',removed);
END $$;
CREATE FUNCTION restrictions.request_identity_history(p_actor bytea,p_membership bytea,p_now bigint)
RETURNS jsonb LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT restrictions.summarize_request_history(coalesce(jsonb_agg(jsonb_build_object(
  'created_on',floor(extract(epoch FROM b.starts_at))::bigint,
  'expires_on',floor(extract(epoch FROM b.ends_at))::bigint,
  'unbanned_on',CASE WHEN b.retirement='unban' THEN floor(extract(epoch FROM b.retired_at))::bigint END,
  'template_id',b.template_id) ORDER BY (b.actor_hash=p_actor) DESC,b.id),'[]'),p_now)
 FROM (SELECT id,actor_hash,starts_at,ends_at,retired_at,retirement,template_id FROM restrictions.bans
  WHERE actor_hash=p_actor OR (p_membership IS NOT NULL AND pass_identity_hash=p_membership)
  ORDER BY (actor_hash=p_actor) DESC,id LIMIT 10001) b
$$;
REVOKE ALL ON FUNCTION restrictions.request_identity_history(bytea,bytea,bigint) FROM PUBLIC;
CREATE INDEX restriction_pass_history ON restrictions.bans(pass_identity_hash,id) WHERE pass_identity_hash IS NOT NULL;
CREATE OR REPLACE FUNCTION staff_identity.janitor_request_queue(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_offset integer)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; rank integer; page jsonb; choices jsonb; counts jsonb; raw_count integer; over_budget boolean; stamp bigint:=floor(extract(epoch FROM clock_timestamp()))::bigint;
BEGIN
 IF restrictions.invoker() NOT IN('board_auth','board_migrator') THEN
  RAISE EXCEPTION 'Request queue is unavailable' USING ERRCODE='42501'; END IF;
 IF (p_board IS NOT NULL AND p_board !~ '^[a-z0-9]{1,10}$') OR p_offset IS NULL OR p_offset NOT BETWEEN 0 AND 100000 THEN
  RAISE EXCEPTION 'Invalid request queue page' USING ERRCODE='23514'; END IF;
 account:=restrictions.message_authority(p_session,p_csrf,p_idle);
 SELECT restrictions.template_rank(a.role) INTO rank FROM staff_identity.accounts a WHERE a.id=account;
 IF rank<10 OR (p_board IS NOT NULL AND NOT staff_identity.has_board_access(account,p_board)) THEN
  RAISE EXCEPTION 'Request queue scope is unavailable' USING ERRCODE='P134A'; END IF;
 PERFORM restrictions.gate(false);
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.requests LIMIT 100001) bounded)>100000
  OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.templates LIMIT 129) bounded)>128 THEN
  RAISE EXCEPTION 'Request queue inventory exceeds its bound' USING ERRCODE='P134C'; END IF;
 -- Source prepends priority rules while traversing its initial order. Reverse
 -- only that subset. Stable IDs resolve the source database's unspecified ties.
 WITH ranked AS (
  SELECT r.id,r.board,r.post_id,r.template_id,coalesce(t.name,r.template_name) AS template_name,
   r.warning,r.account_id,r.created_at,r.payload,r.charged_bytes,r.actor_hash,r.pass_identity_hash,
   coalesce(t.source_rule IN('global1','hm6','s6'),false) AS priority,
   coalesce(t.enabled AND restrictions.template_rank(t.minimum_role)<=rank
    AND t.request_save_policy IN('none','json_only','everything') AND t.request_blacklist_policy IN('none','image','rejectimage')
    AND (EXISTS(SELECT 1 FROM restrictions.request_media m WHERE m.request_id=r.id)
     OR r.warning OR t.request_blacklist_policy='none' OR r.upload_observation IN('known','absent')),false) AS can_accept,
   row_number() OVER(ORDER BY CASE WHEN p_board IS NOT NULL THEN r.template_id END,r.created_at,r.id) AS position
  FROM restrictions.requests r LEFT JOIN restrictions.templates t ON t.id=r.template_id
  WHERE (CASE WHEN p_board IS NULL THEN r.board<>'test' ELSE r.board=p_board END)
   AND staff_identity.has_board_access(account,r.board)
 ), selected AS (
  SELECT *,CASE WHEN priority THEN -position ELSE position END AS item_order FROM ranked
  ORDER BY priority DESC,CASE WHEN priority THEN -position ELSE position END OFFSET p_offset LIMIT 26
 ), numbered AS (
  SELECT *,row_number() OVER(ORDER BY priority DESC,item_order) AS item_number,
   sum(charged_bytes) OVER(ORDER BY priority DESC,item_order) AS page_bytes FROM selected
 ), histories AS MATERIALIZED (
  SELECT actor_hash,pass_identity_hash,restrictions.request_identity_history(actor_hash,pass_identity_hash,stamp) AS history
  FROM (SELECT DISTINCT actor_hash,pass_identity_hash FROM numbered WHERE item_number<=25) actors
 )
 SELECT count(*),coalesce(bool_or(page_bytes>16777216) FILTER(WHERE item_number<=25),false),
  coalesce(jsonb_agg(jsonb_build_object('id',id,'board',board,'post_id',post_id,'template_id',template_id,
   'template_name',template_name,'warning',warning,'account_id',account_id,'created_at',created_at,
   'history',history,'can_accept',can_accept,'media',(SELECT jsonb_build_object('format',m.output_format,'width',m.width,'height',m.height,'thumbnail_width',m.thumbnail_width,'thumbnail_height',m.thumbnail_height) FROM restrictions.request_media m WHERE m.request_id=n.id),'payload',CASE WHEN page_bytes<=16777216 THEN payload::text END)
   ORDER BY priority DESC,item_order) FILTER(WHERE item_number<=25),'[]') INTO raw_count,over_budget,page FROM numbered n LEFT JOIN histories h ON h.actor_hash=n.actor_hash AND h.pass_identity_hash IS NOT DISTINCT FROM n.pass_identity_hash;
 IF over_budget THEN RAISE EXCEPTION 'Request page exceeds its byte budget' USING ERRCODE='P134C'; END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',t.id,'name',t.name,'global',t.global,'warning',t.warning,
  'reason',t.public_reason,'seconds',t.seconds,'no_warn',coalesce(t.source_rule='global1',false),'available',coalesce(t.request_save_policy IN('none','json_only','everything')
   AND t.request_blacklist_policy IN('none','image','rejectimage'),false)) ORDER BY t.id),'[]') INTO choices
  FROM restrictions.templates t WHERE t.enabled AND restrictions.template_rank(t.minimum_role)<=rank;
 SELECT coalesce(jsonb_agg(jsonb_build_object('board',b.board,'count',b.count) ORDER BY b.board),'[]') INTO counts
  FROM (SELECT r.board,count(*) AS count FROM restrictions.requests r
   WHERE staff_identity.has_board_access(account,r.board) GROUP BY r.board) b;
 PERFORM restrictions.message_authority(p_session,p_csrf,p_idle);
 RETURN jsonb_build_object('requests',page,'templates',choices,'counts',counts,'offset',p_offset,
  'previous_offset',greatest(0,p_offset-25),'next_offset',CASE WHEN raw_count=26 THEN p_offset+25 ELSE 0 END);
END $$;
CREATE OR REPLACE FUNCTION staff_identity.janitor_request_history(p_session bytea,p_csrf bytea,p_idle integer,p_request bigint,p_before bigint)
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
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans WHERE actor_hash=target.actor_hash OR (target.pass_identity_hash IS NOT NULL AND pass_identity_hash=target.pass_identity_hash) LIMIT 10001) bounded)>10000 THEN
  RAISE EXCEPTION 'Request history exceeds its bound' USING ERRCODE='P134C'; END IF;
 WITH selected AS (
  SELECT b.* FROM restrictions.bans b WHERE (b.actor_hash=target.actor_hash OR (target.pass_identity_hash IS NOT NULL AND b.pass_identity_hash=target.pass_identity_hash))
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
CREATE FUNCTION content.evaluate_member_posting_restrictions(p_actor bytea,p_token bytea,p_board text,p_request bigint,p_target bigint)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE membership bytea;
BEGIN
 IF restrictions.invoker()<>'board_public' THEN RAISE EXCEPTION 'Public restriction evaluation unavailable' USING ERRCODE='42501'; END IF;
 PERFORM restrictions.gate(false);
 membership:=verification_private.pass_posting_identity(p_board,p_target,p_token);
 RETURN restrictions.evaluate_qualified(p_actor,p_token,p_board,p_request,membership);
END $$;
REVOKE ALL ON FUNCTION content.evaluate_member_posting_restrictions(bytea,bytea,text,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.evaluate_member_posting_restrictions(bytea,bytea,text,bigint,bigint) TO board_public;
CREATE FUNCTION content.assert_inserted_member_restrictions(p_actor bytea,p_token bytea,p_board text,p_request bigint,p_post bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE state text; membership bytea;
BEGIN
 PERFORM restrictions.gate(false);
 membership:=verification_private.pass_inserted_identity(p_post,p_token);
 state:=restrictions.evaluate_qualified(p_actor,p_token,p_board,p_request,membership);
 IF state='warned' THEN RAISE EXCEPTION 'Error: You were warned. You must first view this warning to post again.' USING ERRCODE='P134W'; END IF;
 IF state='banned' THEN RAISE EXCEPTION 'Error: You are banned.' USING ERRCODE='P134B'; END IF;
 IF state IS DISTINCT FROM 'allowed' THEN RAISE EXCEPTION 'Restriction state is unavailable' USING ERRCODE='P134R'; END IF;
END $$;
REVOKE ALL ON FUNCTION content.assert_inserted_member_restrictions(bytea,bytea,text,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.assert_inserted_member_restrictions(bytea,bytea,text,bigint,bigint) TO board_posting_cooldown_owner;
CREATE OR REPLACE FUNCTION content.check_inserted_restriction() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_token text; v_token_hash bytea;
BEGIN
 IF current_user IN('board_migrator','board_thread_transfer_owner') THEN RETURN NEW; END IF;
 IF current_user<>'board_posting_cooldown_owner' THEN
  RAISE EXCEPTION 'Posting restriction registration is unavailable' USING ERRCODE='42501';
 END IF;
 v_token:=current_setting('board.restriction_token',true);
 IF coalesce(v_token,'')<>'' THEN
  IF octet_length(v_token)<>64 OR v_token !~ '^[0-9a-f]{64}$' THEN RAISE EXCEPTION 'Invalid restriction token' USING ERRCODE='23514'; END IF;
  v_token_hash:=decode(v_token,'hex');
 END IF;
 PERFORM content.assert_inserted_member_restrictions(NEW.actor_hash,v_token_hash,NEW.board,NEW.request_at,NEW.post_id);
 RETURN NEW;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,content,staff_identity FROM board_restriction_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction cooldowns changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction cooldowns changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction cooldowns changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction cooldowns changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction cooldowns changed report profile'; END IF; END $$;
