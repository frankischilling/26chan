-- Owned transaction-only board; every mutation is rolled back.
BEGIN;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,
 video_enabled,video_audio,max_video_bytes,max_video_duration,max_video_dimension,max_image_bytes,gif_only)
VALUES('videocheck','Owned video policy','Synthetic',1000,100,100,100,10,100,true,false,1024,120,2048,2048,true);
SET LOCAL ROLE board_attachment_owner;
DO $$ DECLARE args record; actual text; BEGIN
 PERFORM content.check_attachment_board_policy('videocheck',1024,1024,2048,1,'webm','owned.WEBM',120000000,false);
 -- MP4 output has the general cap; WebM retains the extra video output cap.
 PERFORM content.check_attachment_board_policy('videocheck',1024,2048,1,1,'mp4','owned.MP4',1,false);
 FOR args IN SELECT * FROM (VALUES
  (1025,100,1,1,'webm','owned.webm',1::bigint,false,'Error: File too large.'),
  (100,1025,1,1,'webm','owned.webm',1,false,'Error: File too large.'),
  (100,2049,1,1,'mp4','owned.mp4',1,false,'Error: File too large.'),
  (100,100,1,1,'webm','owned.webm',120000001,false,'Error: Maximum allowed video duration is 120 seconds.'),
  (100,100,1,1,'webm','owned.webm',1,true,'Error: Audio streams are not allowed.'),
  (100,100,2049,1,'webm','owned.webm',1,false,'Error: Image resolution is too large.'),
  (100,100,1,1,'webm','owned.mp4',1,false,'Error: Corrupted file or unsupported file type.'),
  (100,100,1,1,'png','owned.webm',NULL,NULL,'Error: Corrupted file or unsupported file type.')
 ) AS t(input,bytes,width,height,format,filename,duration,audio,message) LOOP
  actual:=NULL;
  BEGIN
   PERFORM content.check_attachment_board_policy('videocheck',args.input,args.bytes,args.width,args.height,args.format,args.filename,args.duration,args.audio);
  EXCEPTION WHEN check_violation THEN GET STACKED DIAGNOSTICS actual=MESSAGE_TEXT;
  END;
  IF actual IS DISTINCT FROM args.message THEN RAISE EXCEPTION 'Policy case mismatch: expected %, got %',args.message,actual; END IF;
 END LOOP;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF has_function_privilege('board_public','content.check_attachment_board_policy(text,bigint,bigint,integer,integer,text,text,bigint,boolean)','EXECUTE')
 OR has_function_privilege('board_media','content.check_attachment_board_policy(text,bigint,bigint,integer,integer,text,text,bigint,boolean)','EXECUTE')
 THEN RAISE EXCEPTION 'Private board policy helper is exposed'; END IF;
END $$;
ROLLBACK;
