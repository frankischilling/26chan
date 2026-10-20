-- Owned deleted-post history, with no corresponding live user content.
DO $$ DECLARE role_name text; BEGIN
 FOREACH role_name IN ARRAY ARRAY['board_public','board_staff','board_moderation_retention'] LOOP
  IF has_table_privilege(role_name,'post_secrets.image_upload_history','SELECT,INSERT,UPDATE,DELETE')
   OR has_table_privilege(role_name,'post_secrets.image_history_scopes','SELECT,INSERT,UPDATE,DELETE') THEN
   RAISE EXCEPTION 'Private image history grant leaked to %',role_name; END IF;
 END LOOP;
 IF NOT has_function_privilege('board_moderation_retention','content.expire_image_upload_history()','EXECUTE')
 OR has_function_privilege('board_public','content.expire_image_upload_history()','EXECUTE')
 OR has_function_privilege('board_staff','content.expire_image_upload_history()','EXECUTE') THEN
  RAISE EXCEPTION 'Unexpected image expiry authority'; END IF;
END $$;
INSERT INTO post_secrets.image_upload_history(post_id,scope,digest,accepted_at)
 SELECT 9000000000+i,'f_md5',decode(repeat('ab',16),'hex'),clock_timestamp()-interval '49 hours'
 FROM generate_series(1,1002) i;
INSERT INTO post_secrets.image_upload_history(post_id,scope,digest,accepted_at)
 VALUES(9000001003,'f_md5',decode(repeat('cd',16),'hex'),clock_timestamp()-interval '47 hours'),
 (9000001004,'f_md5',decode(repeat('ef',16),'hex'),clock_timestamp());
SET SESSION AUTHORIZATION board_moderation_retention;
DO $$ BEGIN
 IF content.expire_image_upload_history()<>1000 OR content.expire_image_upload_history()<>2
  OR content.expire_image_upload_history()<>0 THEN
  RAISE EXCEPTION 'Image history expiry bounds differ'; END IF;
END $$;
RESET SESSION AUTHORIZATION;
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.image_upload_history)<>2 THEN
  RAISE EXCEPTION 'Image expiry removed recent history'; END IF;
END $$;
BEGIN;
ALTER FUNCTION content.record_image_upload_history() RESET search_path;
SET SESSION AUTHORIZATION board_moderation_retention;
DO $$ BEGIN
 BEGIN
  PERFORM content.expire_image_upload_history();
  RAISE EXCEPTION 'Image history expiry accepted authority drift';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
RESET SESSION AUTHORIZATION;
ROLLBACK;
