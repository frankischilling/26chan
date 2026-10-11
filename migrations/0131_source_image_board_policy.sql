-- Source MAX_KB, MIN_W, MIN_H, MAX_DIMENSION and GIF_ONLY admission policy.
-- Decoder resource limits remain separate from board policy.
ALTER TABLE content.boards
    ADD COLUMN max_image_bytes bigint NOT NULL DEFAULT 4194304 CHECK (max_image_bytes BETWEEN 1 AND 1073741824),
    ADD COLUMN min_image_width integer NOT NULL DEFAULT 1 CHECK (min_image_width BETWEEN 1 AND 10000),
    ADD COLUMN min_image_height integer NOT NULL DEFAULT 1 CHECK (min_image_height BETWEEN 1 AND 10000),
    ADD COLUMN max_image_dimension integer NOT NULL DEFAULT 10000 CHECK (max_image_dimension BETWEEN 1 AND 10000),
    ADD COLUMN gif_only boolean NOT NULL DEFAULT false;
UPDATE content.boards b SET max_image_bytes=policy.max_image_bytes,min_image_width=policy.min_image_width,min_image_height=policy.min_image_height,max_image_dimension=policy.max_image_dimension,gif_only=policy.gif_only FROM (VALUES
('a',4194304,1,1,10000,false),
('aco',4194304,1,1,10000,false),
('c',4194304,1,1,10000,false),
('d',4194304,1,1,10000,false),
('e',4194304,1,1,10000,false),
('f',10485760,1,1,10000,false),
('g',4194304,1,1,10000,false),
('gif',4194304,1,1,10000,true),
('h',4194304,1,1,10000,false),
('his',4194304,1,1,10000,false),
('hr',8388608,1000,1000,10000,false),
('k',4194304,1,1,10000,false),
('m',4194304,1,1,10000,false),
('n',4194304,1,1,10000,false),
('o',4194304,1,1,10000,false),
('p',5242880,1,1,10000,false),
('r',8388608,1,1,10000,false),
('s',8388608,500,500,8000,false),
('t',4194304,1,1,10000,false),
('u',4194304,1,1,10000,false),
('v',4194304,1,1,10000,false),
('w',6291456,480,600,10000,false),
('wg',6291456,480,600,10000,false),
('i',4194304,1,1,10000,false),
('ic',4194304,1,1,10000,false),
('cm',4194304,1,1,10000,false),
('y',4194304,1,1,10000,false),
('an',4194304,1,1,10000,false),
('cgl',4194304,1,1,10000,false),
('ck',4194304,1,1,10000,false),
('co',4194304,1,1,10000,false),
('fa',4194304,1,1,10000,false),
('fit',4194304,1,1,10000,false),
('jp',4194304,1,1,10000,false),
('mlp',4194304,1,1,10000,false),
('mu',4194304,1,1,10000,false),
('po',8388608,1,1,10000,false),
('sp',4194304,1,1,10000,false),
('tg',8388608,1,1,10000,false),
('toy',4194304,1,1,10000,false),
('trv',8388608,1,1,10000,false),
('tv',4194304,1,1,10000,false),
('x',4194304,1,1,10000,false),
('b',2097152,1,1,10000,false),
('soc',5242880,1,1,8000,false),
('r9k',4194304,1,1,10000,false),
('test',4194304,1,1,10000,false),
('adv',4194304,1,1,10000,false),
('lit',4194304,1,1,10000,false),
('int',4194304,1,1,10000,false),
('sci',4194304,1,1,10000,false),
('3',4194304,1,1,10000,false),
('vp',4194304,1,1,10000,false),
('diy',4194304,1,1,10000,false),
('pol',4194304,1,1,10000,false),
('hc',8388608,500,500,8000,false),
('vg',4194304,1,1,10000,false),
('hm',8388608,1,1,8000,false),
('j',4194304,1,1,10000,false),
('wsg',6291456,1,1,10000,true),
('out',5242880,1,1,8000,false),
('lgbt',4194304,1,1,10000,false),
('vr',4194304,1,1,10000,false),
('gd',8388608,1,1,10000,false),
('s4s',2097152,1,1,10000,false),
('biz',4194304,1,1,10000,false),
('qa',4194304,1,1,10000,false),
('trash',4194304,1,1,10000,false),
('news',4194304,1,1,10000,false),
('wsr',8388608,1,1,10000,false),
('qst',8388608,1,1,10000,false),
('bant',2097152,1,1,10000,false),
('vip',4194304,1,1,10000,false),
('vrpg',4194304,1,1,10000,false),
('vmg',4194304,1,1,10000,false),
('vst',4194304,1,1,10000,false),
('vt',4194304,1,1,10000,false),
('vm',4194304,1,1,10000,false),
('pw',4194304,1,1,10000,false),
('xs',4194304,1,1,10000,false),
('asp',4194304,1,1,10000,false),
('qb',4194304,1,1,10000,false)) policy(slug,max_image_bytes,min_image_width,min_image_height,max_image_dimension,gif_only) WHERE b.slug=policy.slug;

-- Both supported attachment entry points already hold board/thread/job locks.
-- Apply policy to authenticated input length and approved pixels/format.
GRANT SELECT(max_image_bytes,min_image_width,min_image_height,max_image_dimension,gif_only)
    ON content.boards TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE FUNCTION content.check_image_board_policy(
    p_board text,p_input_bytes bigint,p_width integer,p_height integer,p_format text
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE b record;
BEGIN
    SELECT max_image_bytes,min_image_width,min_image_height,max_image_dimension,gif_only
        INTO b FROM content.boards WHERE slug=p_board;
    IF NOT FOUND OR p_input_bytes IS NULL OR p_width IS NULL OR p_height IS NULL
        OR p_format IS NULL OR p_format NOT IN ('png','gif') THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF p_input_bytes<1 OR p_input_bytes>least(b.max_image_bytes,8388608) THEN
        RAISE EXCEPTION 'Error: File too large.' USING ERRCODE='23514';
    END IF;
    IF b.gif_only AND p_format<>'gif' THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    IF p_width<b.min_image_width OR p_height<b.min_image_height THEN
        RAISE EXCEPTION 'Error: Image resolution is too small.' USING ERRCODE='23514';
    END IF;
    IF p_width>b.max_image_dimension OR p_height>b.max_image_dimension THEN
        RAISE EXCEPTION 'Error: Image resolution is too large.' USING ERRCODE='23514';
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.check_image_board_policy(text,bigint,integer,integer,text)
    FROM PUBLIC,board_public,board_staff,board_auth,board_media,board_media_read,board_media_intake,board_monitor;

CREATE OR REPLACE FUNCTION content.insert_post_attachment(
    p_id bigint, p_board text, p_thread bigint, p_name text, p_subject text,
    p_comment text, p_job text, p_capability text, p_spoiler boolean,
    p_posted_at timestamptz
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_limit integer;
    v_sticky boolean;
    v_undead boolean;
    v_state text;
    v_created timestamptz;
    v_filename text;
    v_input_bytes bigint;
    v_hash bytea;
    v_asset record;
BEGIN
    IF p_posted_at IS NULL OR NOT isfinite(p_posted_at) THEN
        RAISE EXCEPTION 'Invalid posting timestamp.' USING ERRCODE = '22023';
    END IF;
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Attachment insertion requires Read Committed.' USING ERRCODE = '22023';
    END IF;
    IF p_job IS NULL OR octet_length(p_job) <> 32 OR p_job !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$'
       OR p_spoiler IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    -- Same lock order as posting and deletion: board, thread, then media job.
    SELECT b.image_limit INTO v_limit FROM content.boards b WHERE b.slug = p_board FOR UPDATE;
    IF v_limit IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    SELECT t.sticky, t.undead INTO v_sticky, v_undead
        FROM content.threads t WHERE t.board = p_board AND t.id = p_thread
        AND NOT t.deleted AND NOT t.closed AND t.archived_at IS NULL FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.created_at, j.filename, j.input_bytes INTO v_state, v_created, v_filename, v_input_bytes
        FROM media.jobs j WHERE j.id = p_job FOR UPDATE;
    SELECT h.capability_hash INTO v_hash FROM media_intake.handles h WHERE h.job_id = p_job;
    -- Recheck expiry after the lock wait. Capabilities are never stored in content.
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR v_created + interval '2 hours' <= clock_timestamp() THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    IF v_state <> 'published' OR EXISTS (SELECT 1 FROM content.post_media m WHERE m.job_id = p_job) THEN
        RAISE EXCEPTION 'Attachment is not ready or was already used.' USING ERRCODE = 'P0001';
    END IF;
    SELECT a.id, a.bytes, a.width, a.height, a.output_format INTO v_asset FROM media.assets a
        WHERE a.job_id = p_job AND a.state = 'approved' ORDER BY a.id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE = 'P0001';
    END IF;
    PERFORM content.check_image_board_policy(p_board,v_input_bytes,v_asset.width,v_asset.height,v_asset.output_format);
    IF v_limit = 0 OR (NOT v_sticky AND NOT v_undead AND p_id <> p_thread AND (SELECT count(*) FROM content.post_media m
        JOIN content.posts p ON p.id = m.post_id WHERE p.board = p_board
        AND p.thread_id = p_thread AND p.id <> p_thread AND NOT p.deleted AND NOT m.file_deleted) >= v_limit) THEN
        RAISE EXCEPTION 'This board or thread cannot accept another image.' USING ERRCODE = 'P0001';
    END IF;
    INSERT INTO content.posts(id, board, thread_id, name, subject, comment, created_at)
        VALUES (p_id, p_board, p_thread, p_name, p_subject, p_comment, date_trunc('second', p_posted_at));
    INSERT INTO content.post_media(post_id, job_id, asset_id, filename, bytes, width, height, spoiler)
        VALUES (p_id, p_job, v_asset.id, v_filename, v_asset.bytes, v_asset.width, v_asset.height, p_spoiler);
END
$$;
REVOKE ALL ON FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz) TO board_public;


CREATE OR REPLACE FUNCTION content.consume_staff_attachment_receipt(
    p_id bigint,p_board text,p_thread bigint,p_job text,p_capability_hash bytea,
    p_spoiler boolean,p_authorized boolean
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE b record; t record; j record; a record; v_hash bytea;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Attachment insertion requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF p_job IS NULL OR octet_length(p_job)<>32 OR p_job !~ '^[0-9a-f]{32}$'
        OR p_capability_hash IS NULL OR octet_length(p_capability_hash)<>32
        OR p_spoiler IS NULL OR p_authorized IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    SELECT image_limit,text_only,staff_only,upload_board,comment_spoiler_cleanup INTO b
        FROM content.boards WHERE slug=p_board FOR UPDATE;
    IF NOT FOUND OR b.staff_only OR b.image_limit=0 THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    SELECT sticky,undead,closed INTO t FROM content.threads
        WHERE board=p_board AND id=p_thread AND NOT deleted AND archived_at IS NULL FOR UPDATE;
    IF NOT FOUND OR (t.closed AND NOT p_authorized) THEN
        RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=p_id AND board=p_board
        AND thread_id=p_thread AND NOT deleted) THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF p_id<>p_thread AND (b.text_only OR b.upload_board) THEN
        RAISE EXCEPTION 'You cannot upload files on this board' USING ERRCODE='23514';
    END IF;
    SELECT state,created_at,filename,input_bytes INTO j FROM media.jobs WHERE id=p_job FOR UPDATE;
    SELECT capability_hash INTO v_hash FROM media_intake.handles WHERE job_id=p_job;
    -- Check the wall clock after waiting for the job, not the statement clock.
    IF v_hash IS NULL OR v_hash<>p_capability_hash OR j.created_at IS NULL
        OR j.created_at+interval '2 hours'<=clock_timestamp() THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF j.state<>'published' OR EXISTS(SELECT 1 FROM content.post_media WHERE job_id=p_job) THEN
        RAISE EXCEPTION 'Attachment is not ready or was already used.' USING ERRCODE='P0001';
    END IF;
    SELECT id,bytes,width,height,output_format INTO a FROM media.assets
        WHERE job_id=p_job AND state='approved' ORDER BY id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE='P0001';
    END IF;
    PERFORM content.check_image_board_policy(p_board,j.input_bytes,a.width,a.height,a.output_format);
    -- Staff rank never bypasses the existing intake and published-file bounds.
    IF j.input_bytes IS NULL OR j.input_bytes NOT BETWEEN 1 AND 8388608
        OR a.bytes NOT BETWEEN 1 AND (CASE a.output_format WHEN 'gif' THEN 20971520 ELSE 5242880 END) THEN
        RAISE EXCEPTION 'Attachment is too large.' USING ERRCODE='22023';
    END IF;
    IF NOT p_authorized AND NOT t.sticky AND NOT t.undead AND p_id<>p_thread
        AND (SELECT count(*) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
            WHERE p.board=p_board AND p.thread_id=p_thread AND p.id<>p_thread
            AND NOT p.deleted AND NOT m.file_deleted)>=b.image_limit THEN
        RAISE EXCEPTION 'This board or thread cannot accept another image.' USING ERRCODE='P0001';
    END IF;
    INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
        VALUES(p_id,p_job,a.id,j.filename,a.bytes,a.width,a.height,p_spoiler AND b.comment_spoiler_cleanup);
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
