-- Synthetic historical approvals and a still-pending reservation.
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,expires_at)
VALUES(repeat('8',32),'synthetic.png','processing',100,1,repeat('9',32),clock_timestamp()+interval '1 hour');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,
 md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height)
VALUES(repeat('6',32),repeat('7',32),repeat('5',32),repeat('a',64),100,50,30,
 'approved',clock_timestamp(),repeat('b',32),repeat('c',64),50,10,6),
 (repeat('4',32),repeat('8',32),repeat('9',32),repeat('a',64),100,50,30,
 'pending',NULL,repeat('b',32),repeat('c',64),50,10,6);
CREATE TABLE public.input_receipt_predecessor AS
SELECT 'asset'::text kind,to_jsonb(a) value FROM media.assets a
UNION ALL SELECT 'job',to_jsonb(j) FROM media.jobs j
UNION ALL SELECT 'approved',to_jsonb(a) FROM media.approved_assets a
UNION ALL SELECT 'post',to_jsonb(p) FROM content.posts p;
REVOKE ALL ON public.input_receipt_predecessor FROM PUBLIC;
