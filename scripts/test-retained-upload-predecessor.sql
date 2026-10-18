-- A historical attachment whose asset was already collected has no recoverable
-- normalized digest. Its consumed job/asset IDs remain useful tombstones.
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
 VALUES(1000002,repeat('19',16),repeat('94',16),'Owned historical file.png',100,10,20,false,true);
CREATE TABLE public.owned_upload_digest_before AS
 SELECT jsonb_agg(to_jsonb(m) ORDER BY post_id) rows FROM content.post_media m;
REVOKE ALL ON public.owned_upload_digest_before FROM PUBLIC;
