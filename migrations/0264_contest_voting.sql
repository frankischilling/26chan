-- Pass-bound contest voting, approved listings and concurrent event schedules.
-- Bootstrap board_contest_owner before this migration.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before contest voting'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before contest voting'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before contest voting'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before contest voting'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before contest voting'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before contest voting'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM submission_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Submission authority drifted before contest voting'; END IF; END $$;
RESET ROLE;
DO $$
DECLARE r record;
BEGIN
 SELECT * INTO r FROM pg_roles WHERE rolname='board_contest_owner';
 IF NOT FOUND OR r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole
  OR r.rolreplication OR r.rolbypassrls OR r.rolconfig IS NOT NULL THEN
  RAISE EXCEPTION 'Contest owner is missing or unsafe';
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
 THEN RAISE EXCEPTION 'Contest owner has unexpected existing authority'; END IF;
END $$;
-- The source accepts this week's boards while last week's boards are voting.
-- Enabled past campaigns also remain available in the previous-events menu.
DROP INDEX submission_private.one_enabled_contest_per_kind;
GRANT USAGE ON SCHEMA submission_private,submission_api TO board_contest_owner;
GRANT SELECT ON submission_private.contest_campaigns,submission_private.contest_votes TO board_contest_owner;
GRANT SELECT(id,campaign_id,board,state,media_id,score) ON submission_private.contest_entries TO board_contest_owner;
GRANT UPDATE(votes) ON submission_private.contest_campaigns TO board_contest_owner;
GRANT UPDATE(score) ON submission_private.contest_entries TO board_contest_owner;
GRANT INSERT ON submission_private.contest_votes TO board_contest_owner;
GRANT EXECUTE ON FUNCTION submission_private.valid_boards(text[]),submission_private.valid_text(text,integer,boolean,boolean),submission_private.valid_email(text,integer) TO board_contest_owner;
SET LOCAL ROLE board_anonymous_owner;
GRANT EXECUTE ON FUNCTION submission_api.consume_pass_vote(bytea,bytea,bytea,bytea,bigint,bigint) TO board_contest_owner;
RESET ROLE;
SET LOCAL ROLE board_submission_owner;
GRANT EXECUTE ON FUNCTION submission_api.ready() TO board_contest_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA submission_api TO board_contest_owner;
SET LOCAL ROLE board_contest_owner;
CREATE FUNCTION submission_api.contest_vote(p_campaign bigint,p_entry bigint,p_receipt bytea,p_peer bytea,p_session bytea,p_context bytea)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE campaign submission_private.contest_campaigns%ROWTYPE; entry record; voter bytea; stamp bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION 'Contest authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_campaign IS NULL OR p_campaign<=0 OR p_entry IS NULL OR p_entry<=0 THEN RAISE EXCEPTION 'Invalid contest vote.' USING ERRCODE='P225V'; END IF;
 SELECT * INTO campaign FROM submission_private.contest_campaigns WHERE id=p_campaign FOR UPDATE;
 stamp:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF NOT FOUND OR NOT campaign.enabled OR stamp<campaign.voting_starts OR stamp>=campaign.voting_ends THEN RAISE EXCEPTION 'Contest voting is closed.' USING ERRCODE='P225V'; END IF;
 SELECT e.id,e.board,e.state,e.score INTO entry FROM submission_private.contest_entries e WHERE e.id=p_entry AND e.campaign_id=p_campaign FOR UPDATE;
 IF NOT FOUND OR entry.state<>1 OR NOT entry.board=ANY(campaign.boards) THEN RAISE EXCEPTION 'Contest entry is unavailable.' USING ERRCODE='P225V'; END IF;
 voter:=submission_api.consume_pass_vote(p_receipt,p_peer,p_session,p_context,p_entry,p_campaign);
 IF EXISTS(SELECT 1 FROM submission_private.contest_votes WHERE campaign_id=p_campaign AND board=entry.board AND voter_hash=voter) THEN RETURN false; END IF;
 IF campaign.votes>=campaign.vote_capacity OR entry.score>=1000000 THEN RAISE EXCEPTION 'Contest vote capacity has been reached.' USING ERRCODE='P225V'; END IF;
 stamp:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF stamp>=campaign.voting_ends THEN RAISE EXCEPTION 'Contest voting is closed.' USING ERRCODE='P225V'; END IF;
 INSERT INTO submission_private.contest_votes(campaign_id,board,voter_hash,entry_id,created_at) VALUES(p_campaign,entry.board,voter,p_entry,stamp);
 UPDATE submission_private.contest_entries SET score=score+1 WHERE id=p_entry;
 UPDATE submission_private.contest_campaigns SET votes=votes+1 WHERE id=p_campaign;
 RETURN true;
END $$;
CREATE FUNCTION submission_api.contest_listing(p_campaign bigint,p_board text,p_offset integer)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE campaign submission_private.contest_campaigns%ROWTYPE; stamp bigint; voting boolean; entries jsonb; next_offset integer;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_campaign IS NULL OR p_campaign<=0 OR p_board IS NULL OR p_board!~'^[a-z0-9]{1,10}$' OR p_offset IS NULL OR p_offset NOT BETWEEN 0 AND 100000 THEN RAISE EXCEPTION 'Invalid contest listing.' USING ERRCODE='P225V'; END IF;
 SELECT * INTO campaign FROM submission_private.contest_campaigns WHERE id=p_campaign;
 stamp:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF NOT FOUND OR NOT campaign.enabled OR stamp<campaign.submission_starts OR NOT p_board=ANY(campaign.boards) THEN RETURN NULL; END IF;
 voting:=stamp>=campaign.voting_starts AND stamp<campaign.voting_ends;
 IF campaign.kind=2 AND NOT voting THEN RETURN jsonb_build_object('campaign',p_campaign,'board',p_board,'voting',false,'visible',false,'entries','[]'::jsonb,'offset',p_offset,'next',NULL); END IF;
 WITH chosen AS (SELECT e.id,e.board,e.media_id,e.score FROM submission_private.contest_entries e
  WHERE e.campaign_id=p_campaign AND e.board=p_board AND e.state=1 ORDER BY e.id DESC OFFSET p_offset LIMIT 26), numbered AS (SELECT *,row_number() OVER(ORDER BY id DESC) position FROM chosen)
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'board',board,'media_id',media_id,'score',CASE WHEN voting THEN NULL ELSE score END) ORDER BY id DESC) FILTER(WHERE position<=25),'[]'::jsonb),
  CASE WHEN count(*)=26 THEN p_offset+25 ELSE NULL END INTO entries,next_offset FROM numbered;
 RETURN jsonb_build_object('campaign',p_campaign,'board',p_board,'voting',voting,'visible',true,'entries',entries,'offset',p_offset,'next',next_offset);
END $$;
REVOKE ALL ON FUNCTION submission_api.contest_vote(bigint,bigint,bytea,bytea,bytea,bytea),submission_api.contest_listing(bigint,text,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.contest_vote(bigint,bigint,bytea,bytea,bytea,bytea),submission_api.contest_listing(bigint,text,integer) TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_contest_owner;
GRANT CREATE ON SCHEMA submission_api TO board_submission_owner;
SET LOCAL ROLE board_submission_owner;
CREATE FUNCTION submission_api.contest_campaign(p_campaign bigint) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE profile submission_private.contest_campaigns%ROWTYPE;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_campaign IS NULL OR p_campaign<=0 THEN RAISE EXCEPTION 'Invalid contest campaign.' USING ERRCODE='P225V'; END IF;
 SELECT * INTO profile FROM submission_private.contest_campaigns WHERE id=p_campaign AND enabled;
 IF NOT FOUND THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('id',profile.id,'kind',profile.kind,'boards',profile.boards,'submission_starts',profile.submission_starts,'submission_ends',profile.submission_ends,
  'voting_starts',profile.voting_starts,'voting_ends',profile.voting_ends,'capacity',profile.capacity,'current_time',floor(extract(epoch FROM clock_timestamp()))::bigint);
END $$;
CREATE FUNCTION submission_api.contest_directory(p_kind smallint,p_before bigint) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE result jsonb; next_id bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_kind IS NULL OR p_kind NOT IN(1,2) OR p_before IS NULL OR p_before<0 THEN RAISE EXCEPTION 'Invalid contest directory.' USING ERRCODE='P225V'; END IF;
 WITH chosen AS (SELECT p.* FROM submission_private.contest_campaigns p WHERE p.kind=p_kind AND p.enabled AND (p_before=0 OR p.id<p_before) ORDER BY p.id DESC LIMIT 17), numbered AS (SELECT *,row_number() OVER(ORDER BY id DESC) position FROM chosen)
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'boards',boards,'submission_starts',submission_starts,'submission_ends',submission_ends,'voting_starts',voting_starts,'voting_ends',voting_ends) ORDER BY id DESC) FILTER(WHERE position<=16),'[]'::jsonb),
  CASE WHEN count(*)=17 THEN min(id) FILTER(WHERE position<=16) ELSE NULL END INTO result,next_id FROM numbered;
 RETURN jsonb_build_object('kind',p_kind,'campaigns',result,'next',next_id,'current_time',floor(extract(epoch FROM clock_timestamp()))::bigint);
END $$;
-- The earlier single-profile reader stays deterministic for existing callers.
CREATE OR REPLACE FUNCTION submission_api.contest_profile(p_kind smallint) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE chosen bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_kind IS NULL OR p_kind NOT IN(1,2) THEN RAISE EXCEPTION 'Invalid contest kind' USING ERRCODE='23514'; END IF;
 SELECT id INTO chosen FROM submission_private.contest_campaigns WHERE enabled AND kind=p_kind ORDER BY id DESC LIMIT 1;
 IF NOT FOUND THEN RETURN NULL; END IF;
 RETURN submission_api.contest_campaign(chosen);
END $$;
REVOKE ALL ON FUNCTION submission_api.contest_campaign(bigint),submission_api.contest_directory(smallint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.contest_campaign(bigint),submission_api.contest_directory(smallint,bigint) TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_submission_owner;
GRANT CREATE ON SCHEMA submission_private,submission_api TO board_submission_owner;
SET LOCAL ROLE board_submission_owner;
CREATE OR REPLACE FUNCTION submission_private.catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT sha256(convert_to(jsonb_build_object(
 'roles',(SELECT jsonb_agg(jsonb_build_array(rolname,rolcanlogin,rolsuper,rolcreatedb,rolcreaterole,
   rolreplication,rolbypassrls,rolinherit,rolconfig) ORDER BY rolname) FROM pg_roles
   WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner','board_staff_post_owner')),
 'auth_delegate',(SELECT jsonb_build_array(pg_get_userbyid(p.proowner),p.proacl::text,pg_get_functiondef(p.oid))
   FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE n.nspname='staff_identity' AND p.proname='lock_session' AND p.pronargs=2
    AND p.proargtypes[0]='bytea'::regtype AND p.proargtypes[1]='integer'::regtype),
 'membership',(SELECT coalesce(jsonb_agg(jsonb_build_array(r.rolname,u.rolname,m.admin_option,m.inherit_option,m.set_option)
   ORDER BY r.rolname,u.rolname),'[]'::jsonb) FROM pg_auth_members m
   JOIN pg_roles r ON r.oid=m.roleid JOIN pg_roles u ON u.oid=m.member
   WHERE r.rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner','board_staff_post_owner') OR u.rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner','board_staff_post_owner')),
 'owner_grants',(SELECT jsonb_agg(value ORDER BY value::text) FROM (
   SELECT jsonb_build_array('relation',pg_get_userbyid(a.grantee),n.nspname,c.relname,a.privilege_type,a.is_grantable) AS value
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(c.relacl) a
    WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))
   UNION ALL SELECT jsonb_build_array('column',pg_get_userbyid(a.grantee),n.nspname,c.relname,col.attname,a.privilege_type,a.is_grantable)
    FROM pg_attribute col JOIN pg_class c ON c.oid=col.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(col.attacl) a
    WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))
   UNION ALL SELECT jsonb_build_array('function',pg_get_userbyid(a.grantee),n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),a.privilege_type,a.is_grantable)
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace,LATERAL aclexplode(p.proacl) a
    WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))
   UNION ALL SELECT jsonb_build_array('schema',pg_get_userbyid(a.grantee),n.nspname,a.privilege_type,a.is_grantable)
    FROM pg_namespace n,LATERAL aclexplode(n.nspacl) a WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))
   UNION ALL SELECT jsonb_build_array('database',pg_get_userbyid(a.grantee),a.privilege_type,a.is_grantable)
    FROM pg_database d,LATERAL aclexplode(d.datacl) a WHERE d.datname=current_database()
     AND a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))
  ) grants),
 'default_acl',(SELECT coalesce(jsonb_agg(jsonb_build_array(pg_get_userbyid(d.defaclrole),n.nspname,d.defaclobjtype,d.defaclacl::text) ORDER BY pg_get_userbyid(d.defaclrole),n.nspname,d.defaclobjtype),'[]'::jsonb)
   FROM pg_default_acl d LEFT JOIN pg_namespace n ON n.oid=d.defaclnamespace
   WHERE d.defaclrole IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))
    OR EXISTS(SELECT 1 FROM aclexplode(d.defaclacl) a WHERE a.grantee IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner')))),
 'schemas',(SELECT jsonb_agg(jsonb_build_array(n.nspname,pg_get_userbyid(n.nspowner),n.nspacl::text) ORDER BY n.nspname)
   FROM pg_namespace n WHERE n.nspname IN('submission_private','submission_api') OR n.nspowner IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))),
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
    WHERE (n.nspname IN('submission_private','submission_api') OR c.relowner IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))) AND c.relkind IN('r','v','m','p')),
 'sequences',(SELECT jsonb_agg(jsonb_build_array(n.nspname,c.relname,pg_get_userbyid(c.relowner),coalesce(c.relacl,acldefault('s',c.relowner))::text,
  format_type(q.seqtypid,NULL),q.seqstart,q.seqincrement,q.seqmax,q.seqmin,q.seqcache,q.seqcycle) ORDER BY n.nspname,c.relname)
  FROM pg_sequence q JOIN pg_class c ON c.oid=q.seqrelid JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN('submission_private','submission_api') OR c.relowner IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner'))),
 'functions',(SELECT jsonb_agg(jsonb_build_array(n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),
   pg_get_userbyid(p.proowner),p.proacl::text,pg_get_functiondef(p.oid)) ORDER BY n.nspname,p.proname,p.oid::regprocedure::text)
   FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE n.nspname IN('submission_private','submission_api') OR p.proowner IN(SELECT oid FROM pg_roles WHERE rolname IN('board_submission_owner','board_submission_review_owner','board_application_owner','board_contest_owner')))
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
   OR pg_has_role(runtime.name,'board_application_owner','MEMBER')
   OR pg_has_role(runtime.name,'board_contest_owner','MEMBER')
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
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest voting changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest voting changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest voting changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest voting changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest voting changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest voting changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest voting authority changed'; END IF; END $$;
RESET ROLE;
