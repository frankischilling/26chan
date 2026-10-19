-- Persistent owned fixture for a populated custody dump/restore.
SET ROLE board_migrator;
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,md5,
 thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height)
SELECT md5('owned-restore-'||post),md5('owned-restore-'||post),md5('owned-restore-'||post),
 encode(sha256(convert_to('full','UTF8')),'hex'),4,200,100,'approved',clock_timestamp(),repeat('a',32),
 encode(sha256(convert_to('thumb','UTF8')),'hex'),5,200,100 FROM generate_series(207005,207006) post;
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
SELECT post,md5('owned-restore-'||post),md5('owned-restore-'||post),'owned restore.png',4,200,100,false
 FROM generate_series(207005,207006) post;
SET ROLE board_restriction_owner;
INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason,request_save_policy,request_blacklist_policy)
VALUES(9109,'Owned restore media','janitor',false,false,86400,'Owned restore','','everything','none');
UPDATE restrictions.templates SET request_blacklist_policy='none' WHERE id=9006;
SET ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207005,9006,false,false,convert_to('full','UTF8'),convert_to('thumb','UTF8')) AS reviewed_request \gset
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207006,9109,false,false,convert_to('full','UTF8'),convert_to('thumb','UTF8'));
SELECT content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,:reviewed_request,true,NULL,NULL,NULL,NULL,NULL);
RESET ROLE;
