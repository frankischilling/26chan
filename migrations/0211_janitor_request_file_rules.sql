-- Review-generated image rules use retained private digests and the existing
-- admission reader. Runtime roles receive no direct table or sequence grants.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request authority drifted before file effects'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA restrictions,content,staff_identity TO board_restriction_owner;
GRANT INSERT,REFERENCES(id),SELECT(id) ON admission.file_rules TO board_restriction_owner;
GRANT USAGE ON SEQUENCE admission.file_rules_id_seq TO board_restriction_owner;
GRANT SELECT,UPDATE(revision) ON admission.policy TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE TABLE restrictions.request_file_rules (
 request_id bigint PRIMARY KEY CHECK(request_id>0),
 rule_id bigint NOT NULL UNIQUE REFERENCES admission.file_rules(id) ON DELETE CASCADE,
 reviewer_id bigint NOT NULL CHECK(reviewer_id>0),
 requested_by bigint NOT NULL CHECK(requested_by>0),
 accepted_template bigint NOT NULL CHECK(accepted_template BETWEEN 1 AND 2147483647),
 ban_id bigint REFERENCES restrictions.bans(id) ON DELETE SET NULL,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp() CHECK(isfinite(created_at))
);
CREATE FUNCTION restrictions.request_file_rule_decision(p_warning boolean,p_raw boolean,p_digest boolean,p_policy text)
RETURNS smallint LANGUAGE plpgsql IMMUTABLE SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF p_warning IS NULL OR p_raw IS NULL OR p_digest IS NULL OR p_policy IS NULL
  OR p_policy NOT IN('none','image','rejectimage') THEN
  RAISE EXCEPTION 'Invalid request file decision' USING ERRCODE='23514'; END IF;
 IF p_warning OR NOT p_raw OR NOT p_digest OR p_policy='none' THEN RETURN NULL; END IF;
 RETURN CASE WHEN p_policy='image' THEN 1 ELSE 0 END;
END $$;
CREATE FUNCTION restrictions.issue_request_file_rule(p_target restrictions.requests,p_preset restrictions.templates,p_account bigint,p_ban bigint,p_warning boolean)
RETURNS bigint LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE mode smallint; rule bigint; description text;
BEGIN
 IF current_user<>'board_restriction_owner' OR p_account IS NULL OR p_ban IS NULL
  OR NOT EXISTS(SELECT 1 FROM restrictions.bans b WHERE b.id=p_ban AND b.account_id=p_account
   AND b.actor_hash=p_target.actor_hash AND b.board=p_target.board AND b.post_id=p_target.post_id
   AND b.template_id=p_preset.id) THEN
  RAISE EXCEPTION 'Request file effect lacks its issued restriction' USING ERRCODE='P134A'; END IF;
 IF p_preset.request_blacklist_policy='none' OR p_warning THEN RETURN NULL; END IF;
 -- Everything capture still requires private custody and is not admitted yet.
 IF p_target.save_policy NOT IN('none','json_only') OR p_target.upload_observation='unknown' THEN
  RAISE EXCEPTION 'Request file evidence is unavailable' USING ERRCODE='P0207'; END IF;
 mode:=restrictions.request_file_rule_decision(p_warning,true,p_target.upload_observation='known',p_preset.request_blacklist_policy);
 IF mode IS NULL THEN RETURN NULL; END IF;
 IF p_target.normalized_upload_md5 IS NULL OR octet_length(p_target.normalized_upload_md5)<>16 THEN
  RAISE EXCEPTION 'Request file digest is unavailable' USING ERRCODE='P0207'; END IF;
 -- Refuse contention rather than wait while holding restriction/request locks.
 PERFORM singleton FROM admission.policy WHERE singleton FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'File admission policy is unavailable' USING ERRCODE='P134R'; END IF;
 IF (SELECT count(*) FROM (SELECT 1 FROM admission.file_rules LIMIT 129) bounded)>=128 THEN
  RAISE EXCEPTION 'Request file rules exceed their bound' USING ERRCODE='P134C'; END IF;
 description:=p_preset.public_reason||' (via ban template, requested by account '||p_target.account_id||', ban id: '||p_ban||')';
 IF octet_length(p_preset.public_reason)>4096 OR octet_length(description)>4096 THEN
  RAISE EXCEPTION 'Request file reason exceeds its bound' USING ERRCODE='P134C'; END IF;
 INSERT INTO admission.file_rules(board,digest,mode,ban_days,quiet,public_reason,description)
 VALUES('',p_target.normalized_upload_md5,mode,CASE WHEN mode=1 THEN -1 ELSE 0 END,false,p_preset.public_reason,description)
 RETURNING id INTO rule;
 INSERT INTO restrictions.request_file_rules(request_id,rule_id,reviewer_id,requested_by,accepted_template,ban_id)
 VALUES(p_target.id,rule,p_account,p_target.account_id,p_preset.id,p_ban);
 RETURN rule;
END $$;
REVOKE ALL ON FUNCTION restrictions.request_file_rule_decision(boolean,boolean,boolean,text),
 restrictions.issue_request_file_rule(restrictions.requests,restrictions.templates,bigint,bigint,boolean) FROM PUBLIC;

CREATE OR REPLACE FUNCTION content.review_janitor_request(p_session bytea,p_csrf bytea,p_idle integer,p_request bigint,
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
   public_reason,private_reason,account_id,charged_bytes,template_id,upload_observation,normalized_upload_md5)
  VALUES(target.board,target.post_id,actor,target.automatic_identity,decision.global,coalesce(decision.seconds=0,false),stamp,
   CASE WHEN decision.seconds IS NULL THEN NULL ELSE stamp+make_interval(secs=>decision.seconds::double precision) END,
   public_reason,private_reason,account,charge,chosen,target.upload_observation,target.normalized_upload_md5) RETURNING id INTO ban;
  INSERT INTO restrictions.request_ban_evidence(ban_id,request_id,payload) VALUES(ban,p_request,target.payload);
  UPDATE restrictions.capacity SET used_bytes=used_bytes+charge WHERE singleton;
  INSERT INTO restrictions.audit(ban_id,action,account_id)
   VALUES(ban,CASE WHEN decision.seconds=0 THEN 'warning' ELSE 'ban' END,account);
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
 WITH gone AS (DELETE FROM restrictions.requests r WHERE r.id=ANY(ids) RETURNING r.charged_bytes)
 SELECT count(*),coalesce(sum(charged_bytes),0) INTO removed,released FROM gone;
 IF removed<>cardinality(ids) THEN RAISE EXCEPTION 'Reviewed request set changed' USING ERRCODE='P134R'; END IF;
 UPDATE restrictions.request_capacity SET used_bytes=used_bytes-released WHERE singleton;
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,target.board,true);
 RETURN jsonb_build_object('changed',true,'ban_id',ban,'cleared',removed);
END $$;

CREATE OR REPLACE FUNCTION staff_identity.janitor_request_queue(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_offset integer)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; rank integer; page jsonb; choices jsonb; counts jsonb; raw_count integer; over_budget boolean;
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
   r.warning,r.account_id,r.created_at,r.payload,r.charged_bytes,
   coalesce(t.source_rule IN('global1','hm6','s6'),false) AS priority,
   coalesce(t.enabled AND restrictions.template_rank(t.minimum_role)<=rank
    AND t.request_save_policy IN('none','json_only') AND t.request_blacklist_policy IN('none','image','rejectimage')
    AND (r.warning OR t.request_blacklist_policy='none' OR r.upload_observation IN('known','absent')),false) AS can_accept,
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
 )
 SELECT count(*),coalesce(bool_or(page_bytes>16777216) FILTER(WHERE item_number<=25),false),
  coalesce(jsonb_agg(jsonb_build_object('id',id,'board',board,'post_id',post_id,'template_id',template_id,
   'template_name',template_name,'warning',warning,'account_id',account_id,'created_at',created_at,
   'can_accept',can_accept,'payload',CASE WHEN page_bytes<=16777216 THEN payload::text END)
   ORDER BY priority DESC,item_order) FILTER(WHERE item_number<=25),'[]') INTO raw_count,over_budget,page FROM numbered;
 IF over_budget THEN RAISE EXCEPTION 'Request page exceeds its byte budget' USING ERRCODE='P134C'; END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',t.id,'name',t.name,'global',t.global,'warning',t.warning,
  'seconds',t.seconds,'available',coalesce(t.request_save_policy IN('none','json_only')
   AND t.request_blacklist_policy IN('none','image','rejectimage'),false)) ORDER BY t.id),'[]') INTO choices
  FROM restrictions.templates t WHERE t.enabled AND restrictions.template_rank(t.minimum_role)<=rank;
 SELECT coalesce(jsonb_agg(jsonb_build_object('board',b.board,'count',b.count) ORDER BY b.board),'[]') INTO counts
  FROM (SELECT r.board,count(*) AS count FROM restrictions.requests r
   WHERE staff_identity.has_board_access(account,r.board) GROUP BY r.board) b;
 PERFORM restrictions.message_authority(p_session,p_csrf,p_idle);
 RETURN jsonb_build_object('requests',page,'templates',choices,'counts',counts,'offset',p_offset,
  'previous_offset',greatest(0,p_offset-25),'next_offset',CASE WHEN raw_count=26 THEN p_offset+25 ELSE 0 END);
END $$;


CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions' OR c.oid IN('post_secrets.posting_history'::regclass,
    'admission.policy'::regclass,'admission.file_rules'::regclass,'admission.file_rules_id_seq'::regclass,
    'admission.automatic_ban_exemptions'::regclass,'admission.dmca_warnings'::regclass,
    'admission.file_effect_capacity'::regclass,'content.posts'::regclass,'content.threads'::regclass,
    'content.reports'::regclass,'post_secrets.report_weight_evidence'::regclass)
 ), triggers AS (
  SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'post_secrets.posting_history'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='post_secrets.posting_history'::regclass AND t.tgname='source_restriction_guard' AND NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'content.reports'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='content.reports'::regclass AND t.tgname='source_report_restriction_guard' AND NOT t.tgisinternal
 ), funcs AS (
  SELECT p.* FROM pg_proc p WHERE p.proowner='board_restriction_owner'::regrole
    OR p.oid IN(SELECT t.tgfoid FROM triggers t)
    OR p.oid IN('post_secrets.janitor_request_unlocked(text,bigint)'::regprocedure,
       'content.report_profile_ready()'::regprocedure,
       'content.report_snapshots_ready()'::regprocedure,
       'content.lock_automatic_range_policy(boolean)'::regprocedure,
       'content.record_inserted_posting_history()'::regprocedure,
       'content.lock_report_network_policy()'::regprocedure,
       'content.report_network_rules(text,bigint,boolean)'::regprocedure,
       'post_secrets.report_network_activity(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
       'post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
       'post_secrets.assert_report_network(text,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,restrictions.acl(coalesce(r.relacl,acldefault(CASE WHEN r.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
       pg_get_expr(d.adbin,d.adrelid),restrictions.acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated)
       ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
    ORDER BY t.identity,t.tgname) FROM triggers t),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),pg_get_functiondef(p.oid),
      restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),
      has_function_privilege('board_restriction_owner',p.oid,'EXECUTE')) ORDER BY p.oid::regprocedure::text)
   FROM pg_proc p WHERE p.oid IN('staff_identity.has_board_access(bigint,text)'::regprocedure,
      'admission.resolve_file_effect_rule(text,bigint,bytea,bytea,boolean)'::regprocedure,
      'admission.file_effect_exempt(inet)'::regprocedure,
      'admission.file_effect_dmca(bytea)'::regprocedure,
      'admission.automatic_ban_exempt(inet)'::regprocedure,
      'admission.record_dmca_warning(bytea,timestamptz)'::regprocedure,
      'content.file_effect_upload_metadata(text,text,text,bigint,boolean)'::regprocedure,
      'content.prior_ban_upload_digest(text,text,text,bigint,boolean,boolean)'::regprocedure,
      'content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)'::regprocedure,
      'content.check_attachment_upload(text,text)'::regprocedure,
      'content.check_attachment_board_policy(text,bigint,bigint,integer,integer,text,text,bigint,boolean)'::regprocedure,
      'content.revalidate_staff_refusal(bytea,bytea,integer,text)'::regprocedure,
      'content.text_thread_transfer_ready()'::regprocedure,
      'content.lock_posting_actor(bytea,boolean)'::regprocedure,
      'post_secrets.lookup_automatic_identity(bytea,bigint)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
      'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),restrictions.acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
   FROM pg_namespace n WHERE n.nspname='restrictions'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,content,staff_identity FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true
 OR content.report_snapshots_ready() IS DISTINCT FROM true
 OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request file effects changed authority'; END IF; END $$;
RESET ROLE;
