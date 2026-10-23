-- Synthetic database metadata, not proof that any movie was parsed or played.
DO $$
DECLARE v_job text; v_token text; v_id text; candidate media.assets; change text;
BEGIN
 v_job:=replace(gen_random_uuid()::text,'-','');
 INSERT INTO media.jobs(id,filename,expires_at) VALUES(v_job,'owned.swf',clock_timestamp()+interval '5 minutes');
 INSERT INTO media.image_context(job_id,board,thread_id,capcode) VALUES(v_job,'f',0,false);
 UPDATE media.jobs SET state='queued',input_bytes=10485760 WHERE id=v_job;
 UPDATE media.jobs SET state='processing',attempts=1,lease_token=replace(gen_random_uuid()::text,'-',''),
  expires_at=clock_timestamp()+interval '90 seconds' WHERE id=v_job RETURNING lease_token INTO v_token;
 v_id:=replace(gen_random_uuid()::text,'-','');
 INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,md5,output_format)
 VALUES(v_id,v_job,v_token,repeat('a',64),10485760,107374182,1,repeat('b',32),'swf');
 SELECT * INTO STRICT candidate FROM media.assets WHERE id=v_id;
 BEGIN
  UPDATE media.jobs SET state='published',output_sha256=repeat('a',64),output_bytes=10485760,expires_at=NULL WHERE id=v_job;
  UPDATE media.assets SET state='approved',approved_at=clock_timestamp() WHERE id=v_id;
  RAISE EXCEPTION 'SWF approved without original receipt';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 PERFORM media.record_input_receipt(v_job,v_token,v_id,repeat('c',64),10485760,decode(repeat('b',32),'hex'),'upload');
 BEGIN
  UPDATE media.jobs SET state='published',output_sha256=repeat('a',64),output_bytes=10485760,expires_at=NULL WHERE id=v_job;
  UPDATE media.assets SET state='approved',approved_at=clock_timestamp() WHERE id=v_id;
  RAISE EXCEPTION 'SWF approved with substituted original bytes';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 DELETE FROM media.assets WHERE id=v_id;
 FOREACH change IN ARRAY ARRAY[
  '{"width":107374183}', '{"height":0}', '{"md5":null}',
  '{"thumbnail_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","thumbnail_bytes":100,"thumbnail_width":1,"thumbnail_height":1}',
  '{"video_duration_micros":1,"video_has_audio":false}'
 ] LOOP
  BEGIN
   INSERT INTO media.assets SELECT * FROM jsonb_populate_record(candidate,change::jsonb);
   RAISE EXCEPTION 'Malformed SWF manifest accepted: %',change;
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 BEGIN
  INSERT INTO media.assets SELECT * FROM jsonb_populate_record(candidate,'{"source_profile":"png-v1"}'::jsonb);
  RAISE EXCEPTION 'SWF acquired image source provenance';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=v_job;
 BEGIN
  INSERT INTO media.assets SELECT candidate.*;
  RAISE EXCEPTION 'Expired SWF lease accepted';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 UPDATE media.jobs SET expires_at=clock_timestamp()+interval '90 seconds' WHERE id=v_job;
 DELETE FROM media.image_context WHERE job_id=v_job;
 BEGIN
  INSERT INTO media.assets SELECT candidate.*;
  RAISE EXCEPTION 'Unbound SWF accepted';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 -- Restore an owned context through a new receiving job, never mutate a context.
 DELETE FROM media.jobs WHERE id=v_job;
 INSERT INTO media.jobs(id,filename,expires_at) VALUES(v_job,'owned.swf',clock_timestamp()+interval '5 minutes');
 INSERT INTO media.image_context(job_id,board,thread_id,capcode) VALUES(v_job,'g',0,false);
 UPDATE media.jobs SET state='queued',input_bytes=10485760 WHERE id=v_job;
 UPDATE media.jobs SET state='processing',attempts=1,lease_token=v_token,expires_at=clock_timestamp()+interval '90 seconds' WHERE id=v_job;
 BEGIN
  INSERT INTO media.assets SELECT candidate.*;
  RAISE EXCEPTION 'Ordinary image board accepted SWF';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 DELETE FROM media.jobs WHERE id=v_job;
 INSERT INTO media.jobs(id,filename,expires_at) VALUES(v_job,'owned.swf',clock_timestamp()+interval '5 minutes');
 INSERT INTO media.image_context(job_id,board,thread_id,capcode) VALUES(v_job,'f',1,false);
 UPDATE media.jobs SET state='queued',input_bytes=10485760 WHERE id=v_job;
 UPDATE media.jobs SET state='processing',attempts=1,lease_token=v_token,expires_at=clock_timestamp()+interval '90 seconds' WHERE id=v_job;
 BEGIN
  INSERT INTO media.assets SELECT candidate.*;
  RAISE EXCEPTION 'Reply accepted SWF';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 DELETE FROM media.jobs WHERE id=v_job;
 INSERT INTO media.jobs(id,filename,expires_at) VALUES(v_job,'owned.swf',clock_timestamp()+interval '5 minutes');
 INSERT INTO media.image_context(job_id,board,thread_id,capcode) VALUES(v_job,'f',0,false);
 UPDATE media.jobs SET state='queued',input_bytes=10485760 WHERE id=v_job;
 UPDATE media.jobs SET state='processing',attempts=1,lease_token=v_token,expires_at=clock_timestamp()+interval '90 seconds' WHERE id=v_job;
 INSERT INTO media.assets SELECT candidate.*;
 PERFORM media.record_input_receipt(v_job,v_token,v_id,repeat('a',64),10485760,decode(repeat('b',32),'hex'),'upload');
 UPDATE media.jobs SET state='published',output_sha256=repeat('a',64),output_bytes=10485760,expires_at=NULL WHERE id=v_job;
  UPDATE media.assets SET state='approved',approved_at=clock_timestamp() WHERE id=v_id;
 IF NOT EXISTS(SELECT 1 FROM media.assets WHERE id=v_id AND state='approved' AND thumbnail_sha256 IS NULL) THEN
  RAISE EXCEPTION 'Exact SWF receipt did not approve'; END IF;
END $$;
