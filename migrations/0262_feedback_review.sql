-- Protected source feedback queues and terminal review actions.
-- Bootstrap board_submission_review_owner before this migration.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before feedback review'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before feedback review'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before feedback review'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before feedback review'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before feedback review'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before feedback review'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM submission_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Submission authority drifted before feedback review'; END IF; END $$;
RESET ROLE;
DO $$
DECLARE r record;
BEGIN
 SELECT * INTO r FROM pg_roles WHERE rolname='board_submission_review_owner';
 IF NOT FOUND OR r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole
  OR r.rolreplication OR r.rolbypassrls OR r.rolconfig IS NOT NULL THEN
  RAISE EXCEPTION 'Submission review owner is missing or unsafe';
 END IF;
 IF (SELECT count(*) FROM pg_auth_members WHERE roleid=r.oid)<>1 OR NOT EXISTS(
  SELECT 1 FROM pg_auth_members m JOIN pg_roles member ON member.oid=m.member
  WHERE m.roleid=r.oid AND member.rolname='board_migrator' AND m.set_option
   AND NOT m.inherit_option AND NOT m.admin_option)
  OR EXISTS(SELECT 1 FROM pg_auth_members WHERE member=r.oid)
  OR EXISTS(SELECT 1 FROM pg_class WHERE relowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_proc WHERE proowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_namespace WHERE nspowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_class c,LATERAL aclexplode(c.relacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_attribute c,LATERAL aclexplode(c.attacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_proc c,LATERAL aclexplode(c.proacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_namespace c,LATERAL aclexplode(c.nspacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_database c,LATERAL aclexplode(c.datacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_default_acl c WHERE c.defaclrole=r.oid)
  OR EXISTS(SELECT 1 FROM pg_default_acl c,LATERAL aclexplode(c.defaclacl) a WHERE a.grantee=r.oid)
 THEN RAISE EXCEPTION 'Submission review owner has unexpected existing authority'; END IF;
END $$;
CREATE TABLE submission_private.feedback_review_log (
 feedback_id bigint PRIMARY KEY REFERENCES submission_private.feedback(id),
 account_id bigint NOT NULL CHECK(account_id>0),
 action text NOT NULL CHECK(action IN('dismiss','approve')),
 before_state smallint NOT NULL CHECK(before_state IN(0,3)),
 after_state smallint NOT NULL CHECK(after_state IN(1,2)),
 created_at bigint NOT NULL CHECK(created_at BETWEEN 0 AND 253402300799),
 CHECK((action='dismiss' AND after_state=2) OR (action='approve' AND before_state=3 AND after_state=1))
);
REVOKE ALL ON submission_private.feedback_review_log FROM PUBLIC,board_public,board_staff,board_auth,board_verifier,board_media,board_media_read,board_media_intake,board_monitor;
GRANT USAGE ON SCHEMA submission_api TO board_auth;
GRANT USAGE ON SCHEMA submission_api TO board_staff_post_owner;
GRANT CREATE ON SCHEMA submission_api TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;
CREATE FUNCTION submission_api.review_authority(p_session bytea,p_csrf bytea,p_idle integer,p_recent boolean)
RETURNS TABLE(account_id bigint,can_dismiss boolean,is_admin boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE identity record;
BEGIN
 IF p_session IS NULL OR octet_length(p_session)<>32 OR p_csrf IS NULL OR octet_length(p_csrf)<>32
  OR p_idle IS NULL OR p_idle NOT BETWEEN 60 AND 3600 OR p_recent IS NULL THEN
  RAISE EXCEPTION 'Feedback review authentication is required' USING ERRCODE='P225A'; END IF;
 SELECT * INTO identity FROM staff_identity.lock_session(p_session,p_idle);
 IF NOT FOUND OR identity.csrf_hash<>p_csrf
  OR NOT coalesce(identity.role='admin' OR 'feedback_review'=ANY(identity.flags),false) THEN
  RAISE EXCEPTION 'Feedback review authentication is required' USING ERRCODE='P225A'; END IF;
 IF p_recent AND identity.recent IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Recent feedback review authentication is required' USING ERRCODE='P225Q'; END IF;
 RETURN QUERY SELECT identity.account_id,coalesce(identity.role='admin' OR 'feedback_dismiss'=ANY(identity.flags),false),identity.role='admin';
END $$;
REVOKE ALL ON FUNCTION submission_api.review_authority(bytea,bytea,integer,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.review_authority(bytea,bytea,integer,boolean) TO board_submission_review_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_staff_post_owner;
GRANT USAGE ON SCHEMA submission_private,submission_api TO board_submission_review_owner;
GRANT SELECT ON submission_private.feedback_profiles,submission_private.feedback_categories,submission_private.feedback TO board_submission_review_owner;
GRANT UPDATE(state,updated_at) ON submission_private.feedback TO board_submission_review_owner;
GRANT INSERT ON submission_private.feedback_review_log TO board_submission_review_owner;
GRANT EXECUTE ON FUNCTION submission_private.valid_text(text,integer,boolean,boolean) TO board_submission_review_owner;
SET LOCAL ROLE board_submission_owner;
GRANT EXECUTE ON FUNCTION submission_api.ready() TO board_submission_review_owner,board_auth;
RESET ROLE;
GRANT CREATE ON SCHEMA submission_api TO board_submission_review_owner;
SET LOCAL ROLE board_submission_review_owner;
CREATE FUNCTION submission_api.feedback_review_page(p_session bytea,p_csrf bytea,p_idle integer,
 p_profile bigint,p_state integer,p_category integer,p_before bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE authority record; profile bigint; rows jsonb; categories jsonb; drafts bigint; next_id bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Feedback review authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_profile IS NULL OR p_profile<0 OR p_state IS NULL OR p_state NOT BETWEEN 0 AND 3
  OR (p_category IS NOT NULL AND p_category NOT BETWEEN 0 AND 65535) OR p_before IS NULL OR p_before<0 THEN
  RAISE EXCEPTION 'Invalid feedback review page' USING ERRCODE='23514'; END IF;
 SELECT * INTO authority FROM submission_api.review_authority(p_session,p_csrf,p_idle,false);
 IF (SELECT count(*) FROM (SELECT 1 FROM submission_private.feedback_profiles LIMIT 10001) bounded)>10000 THEN
  RAISE EXCEPTION 'Feedback profile inventory exceeds its bound' USING ERRCODE='P225C'; END IF;
 IF p_profile=0 THEN SELECT coalesce(max(id) FILTER(WHERE enabled),max(id)) INTO profile FROM submission_private.feedback_profiles;
 ELSE SELECT id INTO profile FROM submission_private.feedback_profiles WHERE id=p_profile; END IF;
 IF profile IS NULL THEN RETURN jsonb_build_object('profile',NULL,'rows','[]'::jsonb,'categories','[]'::jsonb,'draft_count',0,'next',0,'can_dismiss',authority.can_dismiss,'is_admin',authority.is_admin); END IF;
 IF (SELECT count(*) FROM (SELECT 1 FROM submission_private.feedback WHERE profile_id=profile LIMIT 100001) bounded)>100000 THEN
  RAISE EXCEPTION 'Feedback inventory exceeds its bound' USING ERRCODE='P225C'; END IF;
 WITH chosen AS (SELECT f.id,f.category,c.short_label,f.subject,f.message,f.reply,f.reply_capcode,f.updated_at
  FROM submission_private.feedback f JOIN submission_private.feedback_categories c ON c.profile_id=f.profile_id AND c.id=f.category
  WHERE f.profile_id=profile AND f.state=p_state AND (p_category IS NULL OR f.category=p_category)
   AND (p_before=0 OR f.id<p_before) ORDER BY f.id DESC LIMIT 26), numbered AS (
  SELECT *,row_number() OVER(ORDER BY id DESC) position FROM chosen)
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'category',category,'category_label',short_label,
  'subject',subject,'message',message,'reply',reply,'reply_capcode',reply_capcode,'updated_at',updated_at) ORDER BY id DESC)
  FILTER(WHERE position<=25),'[]'::jsonb),CASE WHEN count(*)=26 THEN min(id) FILTER(WHERE position<=25) ELSE 0 END INTO rows,next_id FROM numbered;
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',c.id,'label',c.label,'short_label',c.short_label,
  'count',(SELECT count(*) FROM submission_private.feedback f WHERE f.profile_id=profile AND f.category=c.id AND f.state=0)) ORDER BY c.ordinal),'[]'::jsonb)
  INTO categories FROM submission_private.feedback_categories c WHERE c.profile_id=profile;
 SELECT count(*) INTO drafts FROM submission_private.feedback WHERE profile_id=profile AND state=3;
 PERFORM submission_api.review_authority(p_session,p_csrf,p_idle,false);
 RETURN jsonb_build_object('profile',profile,'rows',rows,'categories',categories,'draft_count',drafts,'next',next_id,
  'can_dismiss',authority.can_dismiss,'is_admin',authority.is_admin);
END $$;
CREATE FUNCTION submission_api.feedback_review_detail(p_session bytea,p_csrf bytea,p_idle integer,p_id bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE result jsonb;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Feedback review authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_id IS NULL OR p_id<=0 THEN RAISE EXCEPTION 'Invalid feedback ID' USING ERRCODE='23514'; END IF;
 PERFORM submission_api.review_authority(p_session,p_csrf,p_idle,false);
 SELECT jsonb_build_object('id',f.id,'profile',f.profile_id,'category',f.category,'category_label',c.short_label,
  'subject',f.subject,'message',f.message,'state',f.state,'reply',f.reply,'reply_capcode',f.reply_capcode,
  'created_at',f.created_at,'updated_at',f.updated_at,'address',host(f.peer),'user_agent',f.user_agent,'language',f.language)
 INTO result FROM submission_private.feedback f JOIN submission_private.feedback_categories c ON c.profile_id=f.profile_id AND c.id=f.category WHERE f.id=p_id;
 PERFORM submission_api.review_authority(p_session,p_csrf,p_idle,false);
 RETURN result;
END $$;
CREATE FUNCTION submission_api.feedback_review_action(p_session bytea,p_csrf bytea,p_idle integer,p_id bigint,p_action text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE authority record; prior smallint; next_state smallint; stamp bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Feedback review authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_id IS NULL OR p_id<=0 OR p_action IS NULL OR p_action NOT IN('approve','dismiss') THEN
  RAISE EXCEPTION 'Invalid feedback review action' USING ERRCODE='23514'; END IF;
 SELECT * INTO authority FROM submission_api.review_authority(p_session,p_csrf,p_idle,true);
 SELECT state INTO prior FROM submission_private.feedback WHERE id=p_id FOR UPDATE;
 IF NOT FOUND THEN RETURN false; END IF;
 IF (prior=0 AND (p_action<>'dismiss' OR NOT authority.can_dismiss))
  OR (prior=3 AND NOT authority.is_admin) THEN
  RAISE EXCEPTION 'Feedback review action is not permitted' USING ERRCODE='P225A'; END IF;
 IF prior NOT IN(0,3) THEN RETURN false; END IF;
 PERFORM submission_api.review_authority(p_session,p_csrf,p_idle,true);
 next_state:=CASE WHEN p_action='approve' THEN 1 ELSE 2 END;
 stamp:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 UPDATE submission_private.feedback SET state=next_state,updated_at=greatest(stamp,updated_at) WHERE id=p_id;
 INSERT INTO submission_private.feedback_review_log(feedback_id,account_id,action,before_state,after_state,created_at)
 VALUES(p_id,authority.account_id,p_action,prior,next_state,stamp);
 RETURN true;
END $$;
REVOKE ALL ON FUNCTION submission_api.feedback_review_page(bytea,bytea,integer,bigint,integer,integer,bigint),
 submission_api.feedback_review_detail(bytea,bytea,integer,bigint),submission_api.feedback_review_action(bytea,bytea,integer,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.feedback_review_page(bytea,bytea,integer,bigint,integer,integer,bigint),
 submission_api.feedback_review_detail(bytea,bytea,integer,bigint),submission_api.feedback_review_action(bytea,bytea,integer,bigint,text) TO board_auth;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_submission_review_owner;
GRANT CREATE ON SCHEMA submission_private,submission_api TO board_submission_owner;
SET LOCAL ROLE board_submission_owner;
CREATE OR REPLACE FUNCTION submission_private.catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT sha256(convert_to(jsonb_build_object(
 'roles',(SELECT jsonb_agg(jsonb_build_array(rolname,rolcanlogin,rolsuper,rolcreatedb,rolcreaterole,
   rolreplication,rolbypassrls,rolinherit,rolconfig) ORDER BY rolname) FROM pg_roles
   WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_staff_post_owner')),
 'auth_delegate',(SELECT jsonb_build_array(pg_get_userbyid(p.proowner),p.proacl::text,pg_get_functiondef(p.oid))
   FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE n.nspname='staff_identity' AND p.proname='lock_session' AND p.pronargs=2
    AND p.proargtypes[0]='bytea'::regtype AND p.proargtypes[1]='integer'::regtype),
 'membership',(SELECT coalesce(jsonb_agg(jsonb_build_array(r.rolname,u.rolname,m.admin_option,m.inherit_option,m.set_option)
   ORDER BY r.rolname,u.rolname),'[]'::jsonb) FROM pg_auth_members m
   JOIN pg_roles r ON r.oid=m.roleid JOIN pg_roles u ON u.oid=m.member
   WHERE r.rolname IN('board_submission_owner','board_submission_review_owner','board_staff_post_owner') OR u.rolname IN('board_submission_owner','board_submission_review_owner','board_staff_post_owner')),
 'owner_grants',(SELECT jsonb_agg(value ORDER BY value::text) FROM (
   SELECT jsonb_build_array('relation',pg_get_userbyid(a.grantee),n.nspname,c.relname,a.privilege_type,a.is_grantable) AS value
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(c.relacl) a
    WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))
   UNION ALL SELECT jsonb_build_array('column',pg_get_userbyid(a.grantee),n.nspname,c.relname,col.attname,a.privilege_type,a.is_grantable)
    FROM pg_attribute col JOIN pg_class c ON c.oid=col.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(col.attacl) a
    WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))
   UNION ALL SELECT jsonb_build_array('function',pg_get_userbyid(a.grantee),n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),a.privilege_type,a.is_grantable)
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace,LATERAL aclexplode(p.proacl) a
    WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))
   UNION ALL SELECT jsonb_build_array('schema',pg_get_userbyid(a.grantee),n.nspname,a.privilege_type,a.is_grantable)
    FROM pg_namespace n,LATERAL aclexplode(n.nspacl) a WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))
   UNION ALL SELECT jsonb_build_array('database',pg_get_userbyid(a.grantee),a.privilege_type,a.is_grantable)
    FROM pg_database d,LATERAL aclexplode(d.datacl) a WHERE d.datname=current_database()
     AND a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))
  ) grants),
 'default_acl',(SELECT coalesce(jsonb_agg(jsonb_build_array(pg_get_userbyid(d.defaclrole),n.nspname,d.defaclobjtype,d.defaclacl::text) ORDER BY pg_get_userbyid(d.defaclrole),n.nspname,d.defaclobjtype),'[]'::jsonb)
   FROM pg_default_acl d LEFT JOIN pg_namespace n ON n.oid=d.defaclnamespace
   WHERE d.defaclrole IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))
    OR EXISTS(SELECT 1 FROM aclexplode(d.defaclacl) a WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner')))),
 'schemas',(SELECT jsonb_agg(jsonb_build_array(n.nspname,pg_get_userbyid(n.nspowner),n.nspacl::text) ORDER BY n.nspname)
   FROM pg_namespace n WHERE n.nspname IN('submission_private','submission_api') OR n.nspowner IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))),
 'relations',(SELECT jsonb_agg(jsonb_build_array(n.nspname,c.relname,c.relkind,pg_get_userbyid(c.relowner),coalesce(c.relacl,acldefault('r',c.relowner))::text,
   c.relrowsecurity,c.relforcerowsecurity,
   (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,a.attgenerated,a.attacl::text,
     pg_get_expr(d.adbin,d.adrelid)) ORDER BY a.attnum) FROM pg_attribute a
    LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
   (SELECT jsonb_agg(pg_get_constraintdef(k.oid) ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=c.oid),
   (SELECT jsonb_agg(pg_get_indexdef(i.indexrelid) ORDER BY ci.relname) FROM pg_index i
    JOIN pg_class ci ON ci.oid=i.indexrelid WHERE i.indrelid=c.oid),
   (SELECT jsonb_agg(pg_get_triggerdef(t.oid) ORDER BY t.tgname) FROM pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal))
    ORDER BY n.nspname,c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE (n.nspname IN('submission_private','submission_api') OR c.relowner IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))) AND c.relkind IN('r','v','m','p')),
 'sequences',(SELECT jsonb_agg(jsonb_build_array(n.nspname,c.relname,pg_get_userbyid(c.relowner),coalesce(c.relacl,acldefault('s',c.relowner))::text,
  format_type(q.seqtypid,NULL),q.seqstart,q.seqincrement,q.seqmax,q.seqmin,q.seqcache,q.seqcycle) ORDER BY n.nspname,c.relname)
  FROM pg_sequence q JOIN pg_class c ON c.oid=q.seqrelid JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN('submission_private','submission_api') OR c.relowner IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner'))),
 'functions',(SELECT jsonb_agg(jsonb_build_array(n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),
   pg_get_userbyid(p.proowner),p.proacl::text,pg_get_functiondef(p.oid)) ORDER BY n.nspname,p.proname,p.oid::regprocedure::text)
   FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE n.nspname IN('submission_private','submission_api') OR p.proowner IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner')))
 )::text,'UTF8'))
$$;
CREATE OR REPLACE FUNCTION submission_api.ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT definition_hash=submission_private.catalog_hash()
  FROM submission_private.catalog WHERE singleton),false)
 AND NOT EXISTS(SELECT 1 FROM (VALUES('board_public'),('board_staff'),('board_auth'),('board_verifier')) runtime(name)
  WHERE has_schema_privilege(runtime.name,'submission_private','USAGE,CREATE')
   OR pg_has_role(runtime.name,'board_submission_owner','MEMBER')
   OR pg_has_role(runtime.name,'board_submission_review_owner','MEMBER')
   OR pg_has_role(runtime.name,'board_migrator','MEMBER'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_private,submission_api FROM board_submission_owner;
SET LOCAL ROLE board_submission_owner;
SELECT set_config('board.submission_catalog_hash',encode(submission_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE submission_private.catalog SET definition_hash=decode(current_setting('board.submission_catalog_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_image_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_image_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_image_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_image_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Feedback review changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Feedback review changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Feedback review changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Feedback review changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Feedback review changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Feedback review changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Feedback review authority changed'; END IF; END $$;
RESET ROLE;
