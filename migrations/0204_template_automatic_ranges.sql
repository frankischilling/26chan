-- Preserve the template-rule predicate independently of local/global ban scope.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before template range production'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
-- NULL means the operator has not supplied source rule evidence. Existing
-- presets must not acquire a derivative effect from their global scope flag.
ALTER TABLE restrictions.templates ADD COLUMN source_rule text
 CHECK(source_rule IS NULL OR octet_length(source_rule)<=255);

CREATE OR REPLACE FUNCTION content.issue_post_template_restriction(
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
 -- Source qualification is the selected template rule, not the ban scope.
 IF NOT preset.warning AND strpos(preset.source_rule,'global')>0 THEN
  PERFORM restrictions.record_automatic_range(p_board,p_post,ban);
 END IF;
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,true);
 RETURN ban;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Template range producer changed restriction authority'; END IF; END $$;
RESET ROLE;
