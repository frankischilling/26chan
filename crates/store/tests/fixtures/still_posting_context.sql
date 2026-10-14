-- Owned IDs remain inside the caller's rollback transaction.
DO $$
DECLARE ordinary text:=replace(gen_random_uuid()::text,'-','');
        badged text:=replace(gen_random_uuid()::text,'-','');
        thread bigint; reply bigint;
BEGIN
    -- This rollback-only fixture tests receipt context, not comment formatting.
    UPDATE content.boards SET word_filter_enabled=false WHERE slug='p';
    INSERT INTO media.jobs(id,filename,expires_at) VALUES
        (ordinary,'synthetic.jpg',clock_timestamp()+interval '5 minutes'),
        (badged,'synthetic.jpg',clock_timestamp()+interval '5 minutes');
    INSERT INTO media.image_context(job_id,board,thread_id,capcode) VALUES
        (ordinary,'p',0,false),(badged,'p',0,true);
    INSERT INTO content.threads(board) VALUES('p') RETURNING id INTO thread;
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
        VALUES(thread,'p',thread,'Anonymous','Owned context fixture','Synthetic image context');
    INSERT INTO content.posts(board,thread_id,name,subject,comment)
        VALUES('p',thread,'Anonymous','','Synthetic reply') RETURNING id INTO reply;
    EXECUTE 'SET LOCAL ROLE board_attachment_owner';
    PERFORM content.check_still_attachment_context(ordinary,'p',thread,thread,'jpeg');
    -- A PNG has no JPEG EXIF-retention choice to bind to badge presence.
    PERFORM content.check_still_attachment_context(badged,'p',thread,thread,'png');
    BEGIN
        PERFORM content.check_still_attachment_context(badged,'p',thread,thread,'jpeg');
        RAISE EXCEPTION 'badged JPEG receipt used for an ordinary post';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        PERFORM content.check_still_attachment_context(ordinary,'aco',thread,thread,'jpeg');
        RAISE EXCEPTION 'receipt crossed boards';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        PERFORM content.check_still_attachment_context(ordinary,'p',reply,thread,'jpeg');
        RAISE EXCEPTION 'OP receipt used for a reply';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    EXECUTE 'RESET ROLE';
END $$;
