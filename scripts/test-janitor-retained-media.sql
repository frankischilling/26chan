-- Owned approved-metadata fixtures test custody, not decoding or public upload admission.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,md5,
 thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height)
VALUES(repeat('ab',16),repeat('ab',16),repeat('ab',16),encode(sha256(convert_to('full','UTF8')),'hex'),4,200,100,
 'approved',clock_timestamp(),repeat('a',32),encode(sha256(convert_to('thumb','UTF8')),'hex'),5,200,100);
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
VALUES(207004,repeat('ab',16),repeat('ab',16),'owned.png',4,200,100,false);
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.templates SET request_blacklist_policy='none' WHERE id=9006;
DO $$ DECLARE role_name text; policy text; has_file boolean; illegal boolean; BEGIN
 FOREACH role_name IN ARRAY ARRAY['board_public','board_auth','board_staff','board_media_read','board_media_intake'] LOOP
  IF has_table_privilege(role_name,'restrictions.request_media','SELECT,INSERT,UPDATE,DELETE')
   OR has_table_privilege(role_name,'restrictions.request_ban_thumbnails','SELECT,INSERT,UPDATE,DELETE')
   OR has_function_privilege(role_name,'restrictions.capture_request_media(bigint,bytea,bytea)','EXECUTE') THEN
   RAISE EXCEPTION 'Private custody leaked to %',role_name; END IF;
 END LOOP;
 FOREACH policy IN ARRAY ARRAY['none','json_only','everything'] LOOP
  FOREACH has_file IN ARRAY ARRAY[false,true] LOOP
   FOREACH illegal IN ARRAY ARRAY[false,true] LOOP
    IF restrictions.request_media_policy(policy,has_file,illegal) IS DISTINCT FROM (policy='everything' AND has_file AND NOT illegal) THEN
     RAISE EXCEPTION 'Source request retention decision differs'; END IF;
   END LOOP;
  END LOOP;
 END LOOP;
END $$;
RESET ROLE;
DO $$ DECLARE approve boolean; req bigint; result jsonb; plan jsonb; value bytea; before_bytes bigint; BEGIN
 FOREACH approve IN ARRAY ARRAY[false,true] LOOP
  BEGIN
   SET LOCAL ROLE board_auth;
   plan:=staff_identity.janitor_request_media_plan(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9006);
   IF plan->>'bytes'<>'4' OR plan->>'thumbnail_bytes'<>'5' THEN RAISE EXCEPTION 'Scoped custody plan differs'; END IF;
   SET LOCAL ROLE board_staff;
   BEGIN
    PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9006,false,false,convert_to('fake','UTF8'),convert_to('thumb','UTF8'));
    RAISE EXCEPTION 'Wrong media digest was retained';
   EXCEPTION WHEN SQLSTATE 'P0207' THEN NULL; END;
   RESET ROLE;
   IF (SELECT deleted FROM content.posts WHERE id=207004)
    OR EXISTS(SELECT 1 FROM restrictions.requests WHERE post_id=207004)
    OR (SELECT used_bytes FROM restrictions.request_media_capacity)<>0 THEN
    RAISE EXCEPTION 'Failed custody left partial effects'; END IF;
   SET LOCAL ROLE board_staff;
   req:=content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9006,false,false,convert_to('full','UTF8'),convert_to('thumb','UTF8'));
   RESET ROLE;
   IF NOT (SELECT deleted FROM content.posts WHERE id=207004)
    OR (SELECT used_bytes FROM restrictions.request_media_capacity)<>1033
    OR EXISTS(SELECT 1 FROM media.approved_post_assets WHERE board='rqown' AND tim=(SELECT tim FROM content.post_media WHERE post_id=207004)) THEN
    RAISE EXCEPTION 'Capture did not revoke public media or charge custody'; END IF;
   SET LOCAL ROLE board_auth;
   SELECT body INTO value FROM staff_identity.janitor_request_media(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,req,false);
   IF value<>convert_to('full','UTF8') THEN RAISE EXCEPTION 'Scoped full image differs'; END IF;
   BEGIN
    PERFORM * FROM staff_identity.janitor_request_media(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,req,false);
    RAISE EXCEPTION 'Janitor read the moderator custody endpoint';
   EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
   SET LOCAL ROLE board_staff;
   result:=content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,req,approve,NULL,NULL,NULL,NULL,NULL);
   RESET ROLE;
   IF EXISTS(SELECT 1 FROM restrictions.request_media WHERE request_id=req)
    OR (SELECT used_bytes FROM restrictions.request_media_capacity)<>(CASE WHEN approve THEN 1029 ELSE 0 END)
    OR (EXISTS(SELECT 1 FROM restrictions.request_ban_thumbnails WHERE source_request_id=req)) IS DISTINCT FROM approve THEN
    RAISE EXCEPTION 'Review custody lifecycle differs'; END IF;
   SET LOCAL ROLE board_auth;
   BEGIN
    PERFORM * FROM staff_identity.janitor_request_media(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,req,true);
    RAISE EXCEPTION 'Cleared request retained its read URL';
   EXCEPTION WHEN SQLSTATE 'P134N' THEN NULL; END;
   RESET ROLE;
   RAISE EXCEPTION 'Rollback owned case' USING ERRCODE='P7999';
  EXCEPTION WHEN SQLSTATE 'P7999' THEN NULL;
  END;
 END LOOP;
END $$;
ROLLBACK;
