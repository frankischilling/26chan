-- Synthetic receipts only. The caller rolls the transaction back.
DO $$
DECLARE v_job text; v_token text; v_id text; candidate media.assets; change text;
BEGIN
    v_job:=replace(gen_random_uuid()::text,'-','');
    INSERT INTO media.jobs(id,filename,expires_at)
        VALUES(v_job,'synthetic.jpg',clock_timestamp()+interval '5 minutes');
    INSERT INTO media.image_context(job_id,board,thread_id,capcode) VALUES(v_job,'p',0,true);
    UPDATE media.jobs SET state='queued',input_bytes=10485760 WHERE id=v_job;
    UPDATE media.jobs SET state='processing',attempts=1,
        lease_token=replace(gen_random_uuid()::text,'-',''),
        expires_at=clock_timestamp()+interval '90 seconds'
        WHERE id=v_job RETURNING lease_token INTO v_token;
    v_id:=replace(gen_random_uuid()::text,'-','');
    INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,
        md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height,
        source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5,
        output_format,still_profile,thumbnail_format,mobile_sha256,mobile_bytes,mobile_width,mobile_height)
    VALUES(v_id,v_job,v_token,repeat('a',64),10485760,10000,10000,
        repeat('b',32),repeat('c',64),1048576,250,250,
        repeat('d',64),10485760,'still-v6',10485760,decode(repeat('b',32),'hex'),
        'jpeg','still-v6','jpeg',repeat('e',64),4194304,1024,1024);
    SELECT * INTO STRICT candidate FROM media.assets WHERE id=v_id;
    -- Immutable even for migration maintenance; old receipts cannot be relabeled.
    FOREACH change IN ARRAY ARRAY[
        'still_profile=NULL','thumbnail_format=''png''',
        'mobile_sha256=NULL','mobile_bytes=1','mobile_width=1','mobile_height=1'
    ] LOOP
        BEGIN
            EXECUTE 'UPDATE media.assets SET '||change||' WHERE id=$1' USING v_id;
            RAISE EXCEPTION 'mutable still receipt: %',change;
        EXCEPTION WHEN insufficient_privilege THEN NULL;
        END;
    END LOOP;
    DELETE FROM media.assets WHERE id=v_id;
    -- Check independent malformed insertion tuples, without an UPDATE trigger masking CHECKs.
    FOREACH change IN ARRAY ARRAY[
        '{"still_profile":null}', '{"still_profile":"unknown"}',
        '{"output_format":"gif"}', '{"thumbnail_format":"png"}',
        '{"thumbnail_bytes":1048577}', '{"bytes":10485761,"source_retained_bytes":10485761}',
        '{"width":10001}', '{"height":10001}', '{"source_profile":null}',
        '{"source_retained_bytes":10485759}', '{"md5":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}',
        '{"mobile_sha256":null}', '{"mobile_bytes":null}', '{"mobile_bytes":4194305}',
        '{"mobile_width":null}', '{"mobile_height":null}', '{"mobile_width":1025}',
        '{"mobile_width":512,"mobile_height":512}'
    ] LOOP
        BEGIN
            INSERT INTO media.assets SELECT * FROM jsonb_populate_record(candidate,change::jsonb);
            RAISE EXCEPTION 'malformed still receipt accepted: %',change;
        EXCEPTION WHEN check_violation THEN NULL;
        END;
    END LOOP;
    -- Source cleanup may grow a JPEG, so retained length is not bounded by input length.
    UPDATE media.jobs SET input_bytes=100 WHERE id=v_job;
    candidate.source_input_bytes:=100;
    INSERT INTO media.assets SELECT candidate.*;
    DELETE FROM media.assets WHERE id=v_id;
    UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=v_job;
    BEGIN
        INSERT INTO media.assets SELECT candidate.*;
        RAISE EXCEPTION 'expired lease accepted';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    UPDATE media.jobs SET expires_at=clock_timestamp()+interval '90 seconds' WHERE id=v_job;
    DELETE FROM media.image_context WHERE job_id=v_job;
    BEGIN
        INSERT INTO media.assets SELECT candidate.*;
        RAISE EXCEPTION 'context-free still manifest accepted';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END $$;
