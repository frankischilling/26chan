-- Owned, disposable pre-0129 fixture. Never run against an application database.
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,
 thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds)
VALUES ('hclean','Owned historical cleanup','Synthetic',2000,1000,1000,100,10,0,0,0),
 ('hother','Owned unrelated history','Synthetic',2000,1000,1000,100,10,0,0,0),
 ('hprivate','Owned private history','Synthetic',2000,1000,1000,100,10,0,0,0);
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT set_config('board.posting_actor',repeat('71',32),true);
INSERT INTO content.threads(id,board) VALUES (31000001,'hclean'),(31001001,'hclean'),
 (31002001,'hother'),(31003001,'hprivate');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT id,'hclean',31000001,'Historical name','Historical subject','Historical body'
 FROM generate_series(31000001,31000131) id;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES
 (31001001,'hclean',31001001,'Live name','Live subject','Live body'),
 (31001002,'hclean',31001001,'Deleted reply','','Deleted reply body'),
 (31002001,'hother',31002001,'Other old name','','Other old body'),
 (31003001,'hprivate',31003001,'Private old name','','Private old body');
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
INSERT INTO post_secrets.deletion(post_id,password_hash)
 SELECT id,'owned-historical-password-proof' FROM content.posts WHERE board IN ('hclean','hother','hprivate');
COMMIT;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at,posts,threads)
VALUES(decode(repeat('72',32),'hex'),decode(repeat('73',32),'hex'),decode(repeat('74',32),'hex'),
 decode(repeat('75',32),'hex'),1,1,1,1,4102444800,135,4);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
 SELECT id,decode(repeat('72',32),'hex'),decode(repeat('76',32),'hex')
 FROM content.posts WHERE board IN ('hclean','hother','hprivate');
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(31000001,'192.0.2.71');
INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES(31000002,31000001);
INSERT INTO staff_identity.accounts(id,role) OVERRIDING SYSTEM VALUE VALUES(31000001,'admin');
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
 VALUES(31000001,'hclean',31000001,'resolve','2020-01-01 UTC');
-- A consumed legacy attachment remains a durable revocation tombstone.
UPDATE content.boards SET image_limit=100 WHERE slug='hclean';
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes)
VALUES(repeat('b1',16),'owned-historical.png','published',1,1,repeat('b2',16),repeat('b3',32),1);
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('b4',16),repeat('b1',16),repeat('b2',16),repeat('b3',32),1,1,1,'approved',clock_timestamp());
INSERT INTO media_intake.handles(job_id,capability_hash)
VALUES(repeat('b1',16),sha256(convert_to(repeat('b5',32),'UTF8')));
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
VALUES(31000001,repeat('b1',16),repeat('b4',16),'owned-historical.png',1,1,1,false);
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
UPDATE content.posts SET deleted=true WHERE id=31001002;
UPDATE content.threads SET deleted=true WHERE id IN(31000001,31002001,31003001);
COMMIT;
-- Simulate already-retained legacy reports, without triggering a new deletion.
INSERT INTO content.reports(id,board,post_id,reason) OVERRIDING SYSTEM VALUE
 SELECT id,'hclean',31000001,'Owned historical report' FROM generate_series(31010001,31010070) id;
INSERT INTO content.reports(id,board,post_id,reason) OVERRIDING SYSTEM VALUE
 VALUES(31010071,'hclean',31001001,'Owned live report'),(31010072,'hother',31002001,'Owned unrelated report');
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
 SELECT id,decode(repeat('72',32),'hex') FROM content.reports WHERE board IN ('hclean','hother');
UPDATE content.boards SET staff_only=true WHERE slug='hprivate';
