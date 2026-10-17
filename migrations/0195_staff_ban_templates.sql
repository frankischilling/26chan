-- Operator-configured restriction presets. No private source template rows are
-- invented, and old bans acquire no inferred template or upload provenance.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before ban templates'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Content catalog drifted before ban templates'; END IF;
END $$;
RESET ROLE;
GRANT SELECT(post_id,file_deleted,normalized_md5) ON content.post_media TO board_restriction_owner;
GRANT SELECT(file_deleted) ON content.posts TO board_restriction_owner;
GRANT CREATE ON SCHEMA restrictions,content,staff_identity TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE OR REPLACE FUNCTION content.restrictions_ready() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname='board_restriction_owner'
    AND NOT(r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.roleid='board_restriction_owner'::regrole
    AND (m.member<>'board_migrator'::regrole OR m.inherit_option OR m.admin_option OR NOT m.set_option))
 AND EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.roleid='board_restriction_owner'::regrole
    AND m.member='board_migrator'::regrole AND NOT m.inherit_option AND NOT m.admin_option AND m.set_option)
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.member='board_restriction_owner'::regrole)
 AND NOT EXISTS(SELECT 1 FROM pg_roles r
    WHERE r.rolname IN('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor')
      AND (pg_has_role(r.oid,'board_posting_cooldown_owner','MEMBER')
        OR pg_has_role(r.oid,'board_thread_transfer_owner','MEMBER')
        OR has_any_column_privilege(r.oid,'post_secrets.posting_history','INSERT,UPDATE')))
 AND NOT has_schema_privilege('board_restriction_owner','restrictions','CREATE')
 AND NOT has_schema_privilege('board_restriction_owner','content','CREATE')
 AND NOT has_schema_privilege('board_restriction_owner','staff_identity','CREATE')
 AND (SELECT definition_hash=restrictions.catalog_hash() FROM restrictions.catalog WHERE singleton)
$$;

CREATE TABLE restrictions.templates (
 id bigint PRIMARY KEY CHECK(id BETWEEN 1 AND 2147483647),
 name text NOT NULL CHECK(octet_length(name) BETWEEN 1 AND 255),
 minimum_role text NOT NULL CHECK(minimum_role IN('janitor','moderator','manager','admin')),
 enabled boolean NOT NULL DEFAULT true,
 global boolean NOT NULL,
 warning boolean NOT NULL,
 seconds bigint,
 public_reason text NOT NULL CHECK(octet_length(public_reason) BETWEEN 1 AND 16384),
 private_reason text NOT NULL CHECK(octet_length(private_reason)<=32768),
 requires_file boolean NOT NULL DEFAULT false,
 -- admin.php:2802 hard-codes these source template IDs as file-required.
 CHECK(id NOT IN(1,123,213) OR requires_file),
 CHECK((warning AND seconds IS NOT NULL AND seconds=0) OR (NOT warning AND (seconds IS NULL OR seconds BETWEEN 1 AND 863913600)))
);
REVOKE ALL ON restrictions.templates FROM PUBLIC;
ALTER TABLE restrictions.bans
 ADD COLUMN template_id bigint CHECK(template_id BETWEEN 1 AND 2147483647),
 ADD COLUMN upload_observation text CHECK(upload_observation IN('absent','unknown','known')),
 ADD COLUMN normalized_upload_md5 bytea CHECK(octet_length(normalized_upload_md5)=16),
 ADD CONSTRAINT restriction_template_snapshot CHECK(
  (template_id IS NULL AND upload_observation IS NULL AND normalized_upload_md5 IS NULL)
  OR (template_id IS NOT NULL AND origin='staff' AND upload_observation IS NOT NULL
   AND ((upload_observation='known' AND normalized_upload_md5 IS NOT NULL)
     OR (upload_observation IN('absent','unknown') AND normalized_upload_md5 IS NULL)))
 );
-- Template IDs are historical values: deleting/reconfiguring a preset must not
-- rewrite a past ban. File-rule IDs and original-input hashes stay separate.
CREATE INDEX restriction_upload_history ON restrictions.bans(normalized_upload_md5,template_id)
 WHERE template_id IN(6,226);

CREATE FUNCTION restrictions.template_rank(p_role text) RETURNS integer
LANGUAGE sql IMMUTABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT CASE p_role WHEN 'janitor' THEN 1 WHEN 'moderator' THEN 10 WHEN 'manager' THEN 20 WHEN 'admin' THEN 50 ELSE 0 END
$$;
CREATE FUNCTION staff_identity.restriction_templates(p_session bytea,p_csrf bytea,p_idle integer,p_board text)
RETURNS TABLE(id bigint,name text,global boolean,warning boolean,seconds bigint,public_reason text) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; rank integer;
BEGIN
 IF restrictions.invoker() NOT IN('board_auth','board_migrator') THEN
  RAISE EXCEPTION 'Restriction templates are unavailable' USING ERRCODE='42501'; END IF;
 account:=restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,false);
 IF current_setting('transaction_isolation')<>'read committed'
   OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction templates are unavailable' USING ERRCODE='P134R'; END IF;
 SELECT restrictions.template_rank(a.role) INTO rank FROM staff_identity.accounts a WHERE a.id=account;
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.templates LIMIT 129) bounded)>128 THEN
  RAISE EXCEPTION 'Restriction template capacity is exhausted' USING ERRCODE='P134C'; END IF;
 RETURN QUERY SELECT t.id,t.name,t.global,t.warning,t.seconds,t.public_reason FROM restrictions.templates t
  WHERE t.enabled AND restrictions.template_rank(t.minimum_role)<=rank ORDER BY t.id LIMIT 128;
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,false);
END $$;

CREATE FUNCTION content.issue_post_template_restriction(
 p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_post bigint,p_template bigint
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE preset restrictions.templates%ROWTYPE; account bigint; rank integer; ban bigint;
 digest bytea; observation text; has_file boolean; charge bigint;
BEGIN
 IF restrictions.invoker() NOT IN('board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Template issuance is unavailable' USING ERRCODE='42501'; END IF;
 IF p_template IS NULL OR p_template NOT BETWEEN 1 AND 2147483647 THEN
  RAISE EXCEPTION 'Invalid restriction template' USING ERRCODE='23514'; END IF;
 account:=restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,true);
 IF content.restrictions_ready() IS DISTINCT FROM true
   OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Template authority is unavailable' USING ERRCODE='P134R'; END IF;
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.templates LIMIT 129) bounded)>128 THEN
  RAISE EXCEPTION 'Restriction template capacity is exhausted' USING ERRCODE='P134C'; END IF;
 SELECT * INTO preset FROM restrictions.templates t WHERE t.id=p_template AND t.enabled FOR SHARE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Invalid restriction template' USING ERRCODE='23514'; END IF;
 SELECT restrictions.template_rank(a.role) INTO rank FROM staff_identity.accounts a WHERE a.id=account;
 IF rank<restrictions.template_rank(preset.minimum_role) THEN
  RAISE EXCEPTION 'Restriction template is not permitted' USING ERRCODE='P134A'; END IF;
 -- The existing issuer owns actor -> capacity -> board/thread/post lock order,
 -- current authority, quota, duration, audit, and exact target revalidation.
 ban:=content.issue_post_restriction(p_session,p_csrf,p_idle,p_board,p_post,
  preset.global,preset.warning,preset.seconds,preset.public_reason,preset.private_reason);
 SELECT m.normalized_md5,NOT m.file_deleted AND NOT p.file_deleted INTO digest,has_file
  FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
  WHERE m.post_id=p_post AND p.board=p_board;
 IF NOT FOUND THEN observation:='absent'; has_file:=false;
 ELSE observation:=CASE WHEN digest IS NULL THEN 'unknown' ELSE 'known' END;
 END IF;
 IF (preset.requires_file OR p_template IN(1,123,213)) AND NOT has_file THEN
  RAISE EXCEPTION 'Restriction template requires a file' USING ERRCODE='23514'; END IF;
 charge:=64+CASE WHEN digest IS NULL THEN 0 ELSE 16 END;
 IF EXISTS(SELECT 1 FROM restrictions.capacity c WHERE c.singleton AND c.used_bytes>c.byte_limit-charge) THEN
  RAISE EXCEPTION 'Restriction capacity is exhausted' USING ERRCODE='P134C'; END IF;
 UPDATE restrictions.bans SET template_id=preset.id,upload_observation=observation,
  normalized_upload_md5=digest,charged_bytes=charged_bytes+charge WHERE id=ban;
 UPDATE restrictions.capacity SET used_bytes=used_bytes+charge WHERE singleton;
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,true);
 RETURN ban;
END $$;
REVOKE ALL ON FUNCTION restrictions.template_rank(text),staff_identity.restriction_templates(bytea,bytea,integer,text),
 content.issue_post_template_restriction(bytea,bytea,integer,text,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.restriction_templates(bytea,bytea,integer,text) TO board_auth;
GRANT EXECUTE ON FUNCTION content.issue_post_template_restriction(bytea,bytea,integer,text,bigint,bigint) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,content,staff_identity FROM board_restriction_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Ban templates changed content authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Ban templates changed restriction authority'; END IF; END $$;
RESET ROLE;
