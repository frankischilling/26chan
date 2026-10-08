-- Staff posts keep their invoker INSERT so the existing proof triggers run.
-- Only a consumed proof can leave a receipt for that new post's AFTER trigger.
ALTER TABLE post_secrets.staff_post_intents
    ADD COLUMN attachment_job text,
    ADD COLUMN attachment_capability_hash bytea,
    ADD COLUMN attachment_spoiler boolean,
    ADD CONSTRAINT staff_post_attachment_context_check CHECK (
        (attachment_job IS NULL AND attachment_capability_hash IS NULL AND attachment_spoiler IS NULL)
        OR (attachment_job IS NOT NULL AND attachment_job ~ '^[0-9a-f]{32}$'
            AND octet_length(attachment_job)=32 AND attachment_capability_hash IS NOT NULL
            AND octet_length(attachment_capability_hash)=32 AND attachment_spoiler IS NOT NULL
            AND board<>'j'));
GRANT UPDATE(attachment_job,attachment_capability_hash,attachment_spoiler)
    ON post_secrets.staff_post_intents TO board_staff_post_owner;

CREATE TABLE post_secrets.staff_attachment_handoffs (
    post_id bigint PRIMARY KEY,
    board text NOT NULL,
    thread_id bigint NOT NULL,
    job_id text NOT NULL,
    capability_hash bytea NOT NULL,
    spoiler boolean NOT NULL,
    authorized_limits boolean NOT NULL,
    account_id bigint NOT NULL,
    session_hash bytea NOT NULL,
    idle_seconds integer NOT NULL,
    expires_at timestamptz NOT NULL,
    transaction_id bigint NOT NULL
);
REVOKE ALL ON post_secrets.staff_attachment_handoffs FROM PUBLIC;
GRANT SELECT,INSERT,DELETE ON post_secrets.staff_attachment_handoffs TO board_staff_post_owner;

GRANT SELECT(staff_only,upload_board,comment_spoiler_cleanup) ON content.boards TO board_attachment_owner;
GRANT SELECT(input_bytes) ON media.jobs TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;

-- Take the media locks before any staff account/session or intent lock.
-- Filename admission already uses this order on the separate posting pool.
CREATE FUNCTION content.lock_staff_attachment_receipt(
    p_board text,p_thread bigint,p_job text,p_capability_hash bytea
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE j record; v_hash bytea;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Attachment insertion requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF p_job IS NULL OR octet_length(p_job)<>32 OR p_job !~ '^[0-9a-f]{32}$'
        OR p_capability_hash IS NULL OR octet_length(p_capability_hash)<>32 THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    PERFORM slug FROM content.boards WHERE slug=p_board AND NOT staff_only FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002'; END IF;
    -- Closed-thread policy depends on the proof, checked after these locks.
    PERFORM id FROM content.threads WHERE board=p_board AND id=p_thread
        AND NOT deleted AND archived_at IS NULL FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='P0002'; END IF;
    SELECT state,created_at INTO j FROM media.jobs WHERE id=p_job FOR UPDATE;
    SELECT capability_hash INTO v_hash FROM media_intake.handles WHERE job_id=p_job;
    IF v_hash IS NULL OR v_hash<>p_capability_hash OR j.created_at IS NULL
        OR j.created_at+interval '2 hours'<=clock_timestamp() THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF j.state<>'published' OR EXISTS(SELECT 1 FROM content.post_media WHERE job_id=p_job) THEN
        RAISE EXCEPTION 'Attachment is not ready or was already used.' USING ERRCODE='P0001';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM media.assets WHERE job_id=p_job AND state='approved') THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE='P0001';
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.lock_staff_attachment_receipt(text,bigint,text,bytea)
    FROM PUBLIC,board_public,board_staff,board_auth;
GRANT EXECUTE ON FUNCTION content.lock_staff_attachment_receipt(text,bigint,text,bytea)
    TO board_staff_post_owner;

-- The only caller is the staff proof owner. Runtime roles cannot attach a
-- receipt to an existing post or supply the moderator bypass themselves.
CREATE FUNCTION content.consume_staff_attachment_receipt(
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
    SELECT id,bytes,width,height INTO a FROM media.assets
        WHERE job_id=p_job AND state='approved' ORDER BY id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE='P0001';
    END IF;
    -- Staff rank never bypasses the existing intake and published-file bounds.
    IF j.input_bytes IS NULL OR j.input_bytes NOT BETWEEN 1 AND 8388608
        OR a.bytes NOT BETWEEN 1 AND 5242880 THEN
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
REVOKE ALL ON FUNCTION content.consume_staff_attachment_receipt(bigint,text,bigint,text,bytea,boolean,boolean)
    FROM PUBLIC,board_public,board_staff,board_auth;
GRANT EXECUTE ON FUNCTION content.consume_staff_attachment_receipt(bigint,text,bigint,text,bytea,boolean,boolean)
    TO board_staff_post_owner;
GRANT EXECUTE ON FUNCTION content.attachment_upload_filename(text,text) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

GRANT CREATE ON SCHEMA content,staff_identity TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;
CREATE FUNCTION staff_identity.bind_post_attachment_context(
    ticket bytea,v_job text,v_capability text,v_spoiler boolean
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF v_job IS NULL OR octet_length(v_job)<>32 OR v_job !~ '^[0-9a-f]{32}$'
        OR v_capability IS NULL OR octet_length(v_capability)<>64 OR v_capability !~ '^[0-9a-f]{64}$'
        OR v_spoiler IS NULL THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    UPDATE post_secrets.staff_post_intents SET attachment_job=v_job,
        attachment_capability_hash=sha256(convert_to(v_capability,'UTF8')),attachment_spoiler=v_spoiler
        WHERE token_hash=ticket AND board<>'j' AND attachment_job IS NULL
        AND attachment_capability_hash IS NULL AND attachment_spoiler IS NULL;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
END $$;
REVOKE ALL ON FUNCTION staff_identity.bind_post_attachment_context(bytea,text,text,boolean)
    FROM PUBLIC,board_public,board_staff,board_auth;

CREATE FUNCTION staff_identity.issue_source_attachment_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,highlight boolean,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_authorized boolean,v_comment_limit integer,v_wordfilter_payload bytea,v_wordfilter_search text,
    v_options text,v_trip text,v_name_allowed boolean,v_raw_name_nonempty boolean,
    v_job text,v_capability text,v_spoiler boolean
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE half_timer boolean;
BEGIN
    half_timer:=staff_identity.issue_source_post_authority(ticket,session_token,csrf,idle,highlight,
        post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time,v_authorized,v_comment_limit,
        v_wordfilter_payload,v_wordfilter_search,v_options,v_trip,v_name_allowed,v_raw_name_nonempty);
    PERFORM staff_identity.bind_post_attachment_context(ticket,v_job,v_capability,v_spoiler);
    RETURN half_timer;
END $$;
CREATE FUNCTION staff_identity.issue_ordinary_attachment_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_authorized boolean,v_comment_limit integer,v_wordfilter_payload bytea,v_wordfilter_search text,
    v_options text,v_trip text,v_name_allowed boolean,v_context jsonb,v_raw_name_nonempty boolean,
    v_job text,v_capability text,v_spoiler boolean
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE half_timer boolean;
BEGIN
    half_timer:=staff_identity.issue_ordinary_post_authority(ticket,session_token,csrf,idle,
        post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time,v_authorized,v_comment_limit,
        v_wordfilter_payload,v_wordfilter_search,v_options,v_trip,v_name_allowed,v_context,v_raw_name_nonempty);
    PERFORM staff_identity.bind_post_attachment_context(ticket,v_job,v_capability,v_spoiler);
    RETURN half_timer;
END $$;
REVOKE ALL ON FUNCTION staff_identity.issue_source_attachment_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,boolean,text,text,boolean),
    staff_identity.issue_ordinary_attachment_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb,boolean,text,text,boolean)
    FROM PUBLIC,board_public,board_staff;
GRANT EXECUTE ON FUNCTION staff_identity.issue_source_attachment_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,boolean,text,text,boolean),
    staff_identity.issue_ordinary_attachment_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb,boolean,text,text,boolean)
    TO board_auth;

-- Keep every existing identity, timer and policy check in the old consumer.
-- Nullable attachment fields also keep pending text proofs usable.
ALTER FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)
    RENAME TO consume_staff_post_authority_without_attachment;
REVOKE ALL ON FUNCTION content.consume_staff_post_authority_without_attachment(bytea,bigint,text,bigint,text,text,text,timestamptz)
    FROM PUBLIC,board_public,board_staff,board_auth;
CREATE FUNCTION content.consume_staff_post_authority(ticket bytea,post_number bigint,v_board text,
    v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE intent post_secrets.staff_post_intents%ROWTYPE; locked_intent post_secrets.staff_post_intents%ROWTYPE; label text;
    job text:=nullif(current_setting('board.staff_attachment_job',true),'');
    capability text:=nullif(current_setting('board.staff_attachment_capability',true),'');
    spoiler text:=nullif(current_setting('board.staff_attachment_spoiler',true),'');
BEGIN
    -- A manually consumed attachment proof must not be paired with a second
    -- proof for the same post, including a text proof with different identity.
    -- This private row survives caller-controlled GUC changes until INSERT.
    IF EXISTS(SELECT 1 FROM post_secrets.staff_attachment_handoffs WHERE post_id=post_number) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT * INTO intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket;
    IF NOT FOUND OR intent.attachment_job IS DISTINCT FROM job
        OR intent.attachment_capability_hash IS DISTINCT FROM sha256(convert_to(capability,'UTF8'))
        OR intent.attachment_spoiler::text IS DISTINCT FROM spoiler THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF intent.attachment_job IS NOT NULL AND (capability !~ '^[0-9a-f]{64}$'
        OR octet_length(capability)<>64 OR EXISTS(SELECT 1 FROM content.posts WHERE id=post_number)) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF intent.attachment_job IS NOT NULL THEN
        PERFORM content.lock_staff_attachment_receipt(v_board,v_thread,
            intent.attachment_job,intent.attachment_capability_hash);
    END IF;
    -- Reuse the established account/session -> intent lock order. Compare the
    -- locked row before handing its attachment fields across the old consumer.
    PERFORM a.id FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE a.id=intent.account_id AND s.token_hash=intent.session_hash FOR SHARE OF a,s;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
    SELECT * INTO locked_intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket FOR UPDATE;
    IF NOT FOUND OR locked_intent IS DISTINCT FROM intent THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    label:=content.consume_staff_post_authority_without_attachment(ticket,post_number,v_board,
        v_thread,v_name,v_subject,v_comment,v_time);
    IF intent.attachment_job IS NOT NULL THEN
        INSERT INTO post_secrets.staff_attachment_handoffs(post_id,board,thread_id,job_id,capability_hash,
            spoiler,authorized_limits,account_id,session_hash,idle_seconds,expires_at,transaction_id)
        VALUES(post_number,v_board,v_thread,intent.attachment_job,intent.attachment_capability_hash,
            intent.attachment_spoiler,NOT intent.is_janitor,intent.account_id,intent.session_hash,
            intent.idle_seconds,intent.expires_at,txid_current());
    END IF;
    RETURN label;
END $$;
REVOKE ALL ON FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)
    FROM PUBLIC,board_public,board_auth;
GRANT EXECUTE ON FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)
    TO board_staff;

CREATE FUNCTION content.attach_staff_post_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE h post_secrets.staff_attachment_handoffs%ROWTYPE;
BEGIN
    DELETE FROM post_secrets.staff_attachment_handoffs WHERE post_id=NEW.id AND board=NEW.board
        AND thread_id=NEW.thread_id AND transaction_id=txid_current() RETURNING * INTO h;
    IF NOT FOUND THEN RETURN NEW; END IF;
    PERFORM content.consume_staff_attachment_receipt(NEW.id,NEW.board,NEW.thread_id,h.job_id,
        h.capability_hash,h.spoiler,h.authorized_limits);
    -- The media lock may outlive the proof, session, idle or recent-auth window.
    -- A failure here rolls back the post, receipt, audit and proof consumption.
    IF h.expires_at<=clock_timestamp() OR NOT EXISTS(
        SELECT 1 FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE a.id=h.account_id AND s.token_hash=h.session_hash AND a.revoked_at IS NULL
            AND a.role IN ('janitor','moderator','manager','admin')
            AND (a.role<>'janitor')=h.authorized_limits AND staff_identity.has_board_access(a.id,h.board)
            AND s.expires_at>clock_timestamp()
            AND s.last_activity_at>clock_timestamp()-make_interval(secs=>h.idle_seconds)
            AND s.authenticated_at>clock_timestamp()-interval '10 minutes') THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    RETURN NEW;
END $$;
CREATE FUNCTION content.reject_orphan_staff_attachment() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM post_secrets.staff_attachment_handoffs
        WHERE post_id=NEW.post_id AND transaction_id=NEW.transaction_id) THEN
        RAISE EXCEPTION 'Staff attachment requires a new post' USING ERRCODE='28000';
    END IF;
    RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION content.attach_staff_post_receipt(),content.reject_orphan_staff_attachment()
    FROM PUBLIC,board_public,board_staff,board_auth;
GRANT EXECUTE ON FUNCTION content.attach_staff_post_receipt(),content.reject_orphan_staff_attachment() TO board_migrator;
RESET ROLE;
CREATE TRIGGER attach_staff_post_receipt AFTER INSERT ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.attach_staff_post_receipt();
CREATE CONSTRAINT TRIGGER reject_orphan_staff_attachment AFTER INSERT ON post_secrets.staff_attachment_handoffs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION content.reject_orphan_staff_attachment();
SET LOCAL ROLE board_staff_post_owner;
REVOKE EXECUTE ON FUNCTION content.attach_staff_post_receipt(),content.reject_orphan_staff_attachment() FROM board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,staff_identity FROM board_staff_post_owner;

-- Resolve the invoker trigger against the new proof wrapper.
CREATE OR REPLACE FUNCTION content.apply_staff_capcode() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE ticket text;
BEGIN
    NEW.capcode:=NULL;
    NEW.staff_authorized_limits:=false;
    PERFORM set_config('board.staff_is_admin','false',true),set_config('board.staff_ordinary_post','false',true);
    IF current_user='board_staff' THEN
        -- Take the final board lock strength before the media helper. A direct
        -- SQL caller must not upgrade a shared board lock after another writer.
        PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE;
        IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
        ticket:=current_setting('board.staff_post_ticket',true);
        IF ticket IS NULL OR ticket !~ '^[0-9a-f]{64}$' THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
        NEW.capcode:=content.consume_staff_post_authority(decode(ticket,'hex'),NEW.id,NEW.board,
            NEW.thread_id,NEW.name,NEW.subject,NEW.comment,NEW.created_at);
        NEW.staff_authorized_limits:=current_setting('board.staff_authorized_limits',true)='true';
        IF current_setting('board.staff_ordinary_post',true) IS DISTINCT FROM 'true' THEN
            PERFORM set_config('board.poster_fingerprint','',true),set_config('board.poster_epoch','',true);
        END IF;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_staff_capcode() FROM PUBLIC;
