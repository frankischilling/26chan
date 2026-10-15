-- The caller rolls back this synthetic manifest and all mutations.
DO $$
DECLARE job text:=replace(gen_random_uuid()::text,'-',''); lease text:=replace(gen_random_uuid()::text,'-','');
        asset text:=replace(gen_random_uuid()::text,'-',''); candidate media.assets; change text;
BEGIN
    INSERT INTO media.jobs(id,filename,expires_at) VALUES(job,'owned.gif',clock_timestamp()+interval '5 minutes');
    INSERT INTO media.image_context(job_id,board,thread_id,capcode) VALUES(job,'gif',0,false);
    UPDATE media.jobs SET state='queued',input_bytes=200 WHERE id=job;
    UPDATE media.jobs SET state='processing',attempts=1,lease_token=lease,
        expires_at=clock_timestamp()+interval '90 seconds' WHERE id=job;
    INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,
        md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height,
        source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5,
        output_format,still_profile,thumbnail_format)
    VALUES(asset,job,lease,repeat('a',64),200,10000,10000,
        repeat('b',32),repeat('c',64),100,250,250,
        repeat('d',64),200,'gif-v7',200,decode(repeat('b',32),'hex'),'gif','gif-v7','jpeg');
    SELECT * INTO STRICT candidate FROM media.assets WHERE id=asset;
    DELETE FROM media.assets WHERE id=asset;
    FOREACH change IN ARRAY ARRAY[
        '{"bytes":8388609,"source_retained_bytes":8388609}',
        '{"width":10001}', '{"height":10001}', '{"output_format":"png"}',
        '{"source_profile":"still-v6"}', '{"still_profile":"still-v6"}',
        '{"still_profile":null}', '{"thumbnail_format":"png"}',
        '{"source_retained_bytes":199}', '{"source_md5":"\\x00000000000000000000000000000000"}',
        '{"mobile_sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","mobile_bytes":100,"mobile_width":1024,"mobile_height":1024}'
    ] LOOP
        BEGIN
            INSERT INTO media.assets SELECT * FROM jsonb_populate_record(candidate,change::jsonb);
            RAISE EXCEPTION 'malformed source GIF manifest accepted: %',change;
        EXCEPTION WHEN check_violation THEN NULL;
        END;
    END LOOP;
    INSERT INTO media.assets SELECT candidate.*;
    BEGIN
        UPDATE media.assets SET thumbnail_format='png',still_profile=NULL WHERE id=asset;
        RAISE EXCEPTION 'GIF manifest could be relabeled';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END $$;

SET LOCAL ROLE board_attachment_owner;
DO $$ BEGIN
    PERFORM content.check_image_board_policy('gif',200,100,100,'gif');
    BEGIN
        PERFORM content.check_image_board_policy('f',200,100,100,'gif');
        RAISE EXCEPTION 'SWF upload board accepted a GIF';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
END $$;
RESET ROLE;
