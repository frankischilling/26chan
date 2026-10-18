-- Atomic request capture and deletion. Private media copying is deliberately
-- unavailable until its separate scoped retention path is installed.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction authority drifted before request storage'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Content authority drifted before request storage'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Snapshot authority drifted before request storage'; END IF;
END $$;
GRANT EXECUTE ON FUNCTION content.report_snapshots_ready() TO board_restriction_owner;
RESET ROLE;
GRANT SELECT ON content.report_snapshot_source TO board_restriction_owner;
GRANT UPDATE(deleted) ON content.posts TO board_restriction_owner;
GRANT SELECT(sticky),UPDATE(deleted) ON content.threads TO board_restriction_owner;
GRANT CREATE ON SCHEMA restrictions,content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;

CREATE TABLE restrictions.request_capacity (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 byte_limit bigint NOT NULL DEFAULT 67108864 CHECK(byte_limit BETWEEN 1 AND 1073741824),
 used_bytes bigint NOT NULL DEFAULT 0 CHECK(used_bytes BETWEEN 0 AND byte_limit),
 row_limit integer NOT NULL DEFAULT 10000 CHECK(row_limit BETWEEN 1 AND 100000)
);
INSERT INTO restrictions.request_capacity(singleton) VALUES(true);
CREATE TABLE restrictions.requests (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK(id>0),
 board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$' AND board<>'j'),
 post_id bigint NOT NULL CHECK(post_id>0),
 thread_id bigint NOT NULL CHECK(thread_id>0),
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 automatic_identity uuid,
 template_id bigint NOT NULL CHECK(template_id BETWEEN 1 AND 2147483647),
 template_name text NOT NULL CHECK(octet_length(template_name) BETWEEN 1 AND 255),
 global boolean NOT NULL,
 warning boolean NOT NULL,
 save_policy text NOT NULL CHECK(save_policy IN('none','json_only')),
 account_id bigint NOT NULL CHECK(account_id>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp() CHECK(isfinite(created_at)),
 upload_observation text NOT NULL CHECK(upload_observation IN('absent','unknown','known')),
 normalized_upload_md5 bytea CHECK(octet_length(normalized_upload_md5)=16),
 version smallint NOT NULL DEFAULT 1 CHECK(version=1),
 payload jsonb NOT NULL CHECK(jsonb_typeof(payload)='object'),
 charged_bytes bigint NOT NULL CHECK(charged_bytes BETWEEN 1024 AND 33555456),
 UNIQUE(board,post_id),
 CHECK((upload_observation='known')=(normalized_upload_md5 IS NOT NULL)),
 CHECK(payload ?& ARRAY['board','post_id','thread_id']
  AND jsonb_typeof(payload->'board')='string' AND jsonb_typeof(payload->'post_id')='number'
  AND jsonb_typeof(payload->'thread_id')='number'
  AND payload->>'board'=board AND (payload->>'post_id')::bigint=post_id
  AND (payload->>'thread_id')::bigint=thread_id),
 CHECK(charged_bytes=octet_length(payload::text)+1024)
);
CREATE INDEX request_actor_template ON restrictions.requests(actor_hash,template_id,id);
CREATE INDEX request_board_queue ON restrictions.requests(board,template_id,created_at,id);
REVOKE ALL ON restrictions.requests,restrictions.requests_id_seq,restrictions.request_capacity FROM PUBLIC;

CREATE FUNCTION restrictions.request_authority(p_session bytea,p_csrf bytea,p_idle integer)
RETURNS bigint LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint;
BEGIN
 account:=restrictions.message_authority(p_session,p_csrf,p_idle);
 IF NOT EXISTS(SELECT 1 FROM staff_identity.sessions s WHERE s.token_hash=p_session
  AND s.authenticated_at>clock_timestamp()-interval '10 minutes') THEN
  RAISE EXCEPTION 'Recent authentication is required' USING ERRCODE='P134Q'; END IF;
 RETURN account;
END $$;

CREATE FUNCTION content.create_janitor_request(p_session bytea,p_csrf bytea,p_idle integer,
 p_board text,p_post bigint,p_template bigint,p_warning boolean,p_async boolean)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp
 SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE account bigint; rank integer; actor bytea; automatic uuid; target_thread bigint;
 preset restrictions.templates%ROWTYPE; saved jsonb; stamp timestamptz; charge bigint; request_id bigint;
 digest bytea; observation text; has_file boolean; warning_request boolean; sticky boolean;
BEGIN
 IF restrictions.invoker() NOT IN('board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Request creation is unavailable' USING ERRCODE='42501'; END IF;
 IF p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_board='j'
  OR p_post IS NULL OR p_post<=0 OR p_template IS NULL OR p_template NOT BETWEEN 1 AND 2147483647
  OR p_warning IS NULL OR p_async IS NULL THEN
  RAISE EXCEPTION 'Invalid request target' USING ERRCODE='23514'; END IF;
 account:=restrictions.request_authority(p_session,p_csrf,p_idle);
 SELECT restrictions.template_rank(a.role) INTO rank FROM staff_identity.accounts a WHERE a.id=account;
 SELECT h.actor_hash,h.automatic_identity INTO actor,automatic FROM post_secrets.posting_history h
  WHERE h.board=p_board AND h.post_id=p_post;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post identity is unavailable' USING ERRCODE='P134N'; END IF;
 PERFORM content.lock_posting_actor(actor,false);
 PERFORM restrictions.gate();
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true
  OR content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request content authority is unavailable' USING ERRCODE='P134R'; END IF;
 PERFORM singleton FROM restrictions.request_capacity WHERE singleton FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Request capacity is unavailable' USING ERRCODE='P134R'; END IF;
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 IF NOT staff_identity.has_board_access(account,p_board)
  AND NOT post_secrets.janitor_request_unlocked(p_board,p_post) THEN
  RAISE EXCEPTION 'Request board access is unavailable' USING ERRCODE='P134A'; END IF;
 SELECT p.thread_id INTO target_thread FROM content.posts p WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 SELECT t.sticky INTO sticky FROM content.threads t WHERE t.board=p_board AND t.id=target_thread
  AND NOT t.deleted AND t.archived_at IS NULL FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 IF p_post=target_thread AND sticky AND rank<50 THEN
  RAISE EXCEPTION 'Sticky thread removal requires an administrator' USING ERRCODE='P134A'; END IF;
 PERFORM p.id FROM content.posts p JOIN post_secrets.posting_history h ON h.board=p.board AND h.post_id=p.id
  WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted AND p.thread_id=target_thread
   AND h.actor_hash=actor AND h.automatic_identity IS NOT DISTINCT FROM automatic FOR UPDATE OF p NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post identity changed' USING ERRCODE='P134N'; END IF;
 SELECT * INTO preset FROM restrictions.templates t WHERE t.id=p_template AND t.enabled FOR SHARE NOWAIT;
 IF NOT FOUND OR restrictions.template_rank(preset.minimum_role)>rank THEN
  RAISE EXCEPTION 'Request template is unavailable' USING ERRCODE='P134A'; END IF;
 IF preset.request_save_policy IS NULL OR preset.request_save_policy='everything' THEN
  RAISE EXCEPTION 'Request retention policy is unavailable' USING ERRCODE='P0207'; END IF;
 IF p_warning AND NOT p_async AND NOT preset.request_can_warn THEN
  RAISE EXCEPTION 'Warning request is not permitted' USING ERRCODE='P134A'; END IF;
 warning_request:=preset.warning OR (p_warning AND NOT p_async);
 SELECT m.normalized_md5,NOT m.file_deleted AND NOT p.file_deleted INTO digest,has_file
  FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
  WHERE p.board=p_board AND p.id=p_post;
 IF NOT FOUND THEN observation:='absent'; has_file:=false;
 ELSE observation:=CASE WHEN digest IS NULL THEN 'unknown' ELSE 'known' END;
 END IF;
 IF preset.requires_file AND NOT has_file THEN
  RAISE EXCEPTION 'Request template requires a file' USING ERRCODE='23514'; END IF;
 SELECT s.payload INTO saved FROM content.report_snapshot_source s WHERE s.board=p_board AND s.post_id=p_post;
 IF NOT FOUND OR saved IS NULL OR octet_length(saved::text)>33554432 THEN
  RAISE EXCEPTION 'Request snapshot is unavailable' USING ERRCODE='P134C'; END IF;
 charge:=octet_length(saved::text)::bigint+1024;
 IF EXISTS(SELECT 1 FROM restrictions.request_capacity c WHERE c.singleton AND
  (c.used_bytes>c.byte_limit-charge OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.requests LIMIT 100001) q)>=c.row_limit)) THEN
  RAISE EXCEPTION 'Request capacity is exhausted' USING ERRCODE='P134C'; END IF;
 IF p_post=target_thread AND (SELECT count(*) FROM (
  SELECT 1 FROM content.posts WHERE board=p_board AND thread_id=target_thread AND NOT deleted LIMIT 10001
 ) bounded)>10000 THEN
  RAISE EXCEPTION 'Request thread deletion exceeds its bound' USING ERRCODE='P134C'; END IF;
 stamp:=clock_timestamp();
 INSERT INTO restrictions.requests(board,post_id,thread_id,actor_hash,automatic_identity,template_id,template_name,
  global,warning,save_policy,account_id,created_at,upload_observation,normalized_upload_md5,payload,charged_bytes)
 VALUES(p_board,p_post,target_thread,actor,automatic,preset.id,preset.name,preset.global,warning_request,
  preset.request_save_policy,account,stamp,observation,digest,saved,charge) RETURNING id INTO request_id;
 UPDATE restrictions.request_capacity SET used_bytes=used_bytes+charge WHERE singleton;
 UPDATE content.posts SET deleted=true WHERE board=p_board
  AND ((p_post=target_thread AND thread_id=target_thread) OR id=p_post) AND NOT deleted;
 UPDATE content.threads SET deleted=deleted OR p_post=target_thread,modified_at=clock_timestamp()
  WHERE board=p_board AND id=target_thread;
 -- The source's undefined metadata variable emits no automatic-range event.
 -- Recheck time-based authority after all content effects; any failure rolls
 -- back capture, capacity and deletion together. Locked board/account state
 -- preserves the access proof without re-reading an already-deleted target.
 PERFORM restrictions.request_authority(p_session,p_csrf,p_idle);
 RETURN request_id;
END $$;
REVOKE ALL ON FUNCTION restrictions.request_authority(bytea,bytea,integer),
 content.create_janitor_request(bytea,bytea,integer,text,bigint,bigint,boolean,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.create_janitor_request(bytea,bytea,integer,text,bigint,bigint,boolean,boolean) TO board_staff;
CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions' OR c.oid IN('post_secrets.posting_history'::regclass,
    'admission.policy'::regclass,'admission.file_rules'::regclass,
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
REVOKE CREATE ON SCHEMA restrictions,content FROM board_restriction_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request storage changed content authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request storage changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request storage changed restriction authority'; END IF; END $$;
RESET ROLE;
