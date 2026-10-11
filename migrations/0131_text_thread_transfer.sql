-- One destructive, media-free thread transfer. Auth prepares and seals trusted
-- Rust formatting; staff can consume only the immutable sealed capability.
-- No historical rewrite, new author identity, ordinary posting, or media reuse.
ALTER TABLE content.posts
    ADD COLUMN saved_presentation bytea,
    ADD COLUMN saved_presentation_search text;
ALTER TABLE content.posts ADD CONSTRAINT posts_saved_presentation_check CHECK (
    (saved_presentation IS NULL AND saved_presentation_search IS NULL)
    OR (saved_presentation IS NOT NULL AND saved_presentation_search IS NOT NULL
        AND wordfilter_payload IS NULL AND wordfilter_search IS NULL
        AND (comment_format BETWEEN 104 AND 111 OR comment_format BETWEEN 120 AND 127)
        AND octet_length(saved_presentation) BETWEEN 9 AND
            CASE WHEN staff_authorized_limits THEN 524288 ELSE 131072 END
        AND substring(saved_presentation FROM 1 FOR 4)=decode(
            CASE WHEN staff_authorized_limits THEN '46503032' ELSE '46503031' END,'hex')
        AND get_byte(saved_presentation,4) BETWEEN 0 AND 1
        AND octet_length(saved_presentation_search)<=
            CASE WHEN staff_authorized_limits THEN 524288 ELSE 131072 END
        AND octet_length(comment)<=CASE WHEN staff_authorized_limits THEN 2097152 ELSE 524288 END));
ALTER TABLE content.posts DROP CONSTRAINT posts_comment_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_check CHECK (
    (saved_presentation IS NOT NULL
        AND octet_length(comment)<=CASE WHEN staff_authorized_limits THEN 2097152 ELSE 524288 END)
    OR (saved_presentation IS NULL AND (
        (wordfilter_payload IS NULL AND char_length(comment) BETWEEN 0 AND
            CASE WHEN staff_authorized_limits THEN 200000 ELSE 16000 END
            AND octet_length(comment)<=CASE WHEN staff_authorized_limits THEN 200000 ELSE 64000 END)
        OR (wordfilter_payload IS NOT NULL AND octet_length(comment)<=
            CASE WHEN staff_authorized_limits THEN 2097152 ELSE 524288 END))));
ALTER TABLE content.posts ADD CONSTRAINT posts_erased_presentation CHECK (
    NOT content_erased OR (saved_presentation IS NULL AND saved_presentation_search IS NULL));

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check CHECK (
    action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler',
        'undead','unundead','thread-options','force-archive','reporter-clear','report-group-clear',
        'text-thread-transfer'));

GRANT USAGE ON SCHEMA content,post_secrets,staff_identity TO board_thread_transfer_owner;
GRANT SELECT(slug,staff_only,upload_board,country_flags,reply_limit,thread_limit),UPDATE(slug)
    ON content.boards TO board_thread_transfer_owner;
GRANT SELECT(id,board,created_at,bumped_at,modified_at,http_modified_at,reply_count,sticky,sticky_rank,
    closed,permasage,permaage,undead,archived_at,archive_expires_at,deleted,content_erased),
    INSERT(id,board,created_at,bumped_at,modified_at,http_modified_at,reply_count,sticky,sticky_rank,
    closed,permasage,permaage,undead,archived_at,archive_expires_at,deleted,content_erased),
    UPDATE(id,deleted) ON content.threads TO board_thread_transfer_owner;
GRANT SELECT(id,board,thread_id,name,trip,subject,comment,created_at,deleted,content_erased,capcode,
    poster_id,json_op_poster_id,country,country_name,board_flag,board_flag_type,flag_name,image_spoiler,
    file_deleted,comment_format,staff_authorized_limits,wordfilter_payload,wordfilter_search,
    drawing_time_seconds,drawing_source_post_id,dice_result,fortune_text,fortune_color,
    saved_presentation,saved_presentation_search),
    INSERT(id,board,thread_id,name,trip,subject,comment,created_at,capcode,poster_id,json_op_poster_id,
    country,country_name,board_flag,board_flag_type,flag_name,image_spoiler,file_deleted,comment_format,
    staff_authorized_limits,saved_presentation,saved_presentation_search),UPDATE(id)
    ON content.posts TO board_thread_transfer_owner;
GRANT SELECT(post_id) ON content.post_media TO board_thread_transfer_owner;
GRANT USAGE ON SEQUENCE content.post_number,content.moderation_audit_id_seq TO board_thread_transfer_owner;
GRANT INSERT(account_id,board,target_id,action),SELECT(id) ON content.moderation_audit TO board_thread_transfer_owner;
GRANT EXECUTE ON FUNCTION content.board_flag_label(text,text),
    staff_identity.has_board_access(bigint,text) TO board_thread_transfer_owner;
GRANT SELECT(id,role,revoked_at,allow_boards,deny_boards),UPDATE(id)
    ON staff_identity.accounts TO board_thread_transfer_owner;
GRANT SELECT(token_hash,csrf_hash,account_id,credential_id,authenticated_at,expires_at,last_activity_at),
    UPDATE(token_hash) ON staff_identity.sessions TO board_thread_transfer_owner;
GRANT SELECT(id,account_id),UPDATE(id) ON staff_identity.credentials TO board_thread_transfer_owner;
GRANT SELECT(post_id,password_hash),INSERT(post_id,password_hash),UPDATE(post_id)
    ON post_secrets.deletion TO board_thread_transfer_owner;
GRANT SELECT(thread_id,peer),INSERT(thread_id,peer),UPDATE(thread_id)
    ON post_secrets.op_peers TO board_thread_transfer_owner;
GRANT SELECT(post_id,thread_id),INSERT(post_id,thread_id),UPDATE(post_id)
    ON post_secrets.op_replies TO board_thread_transfer_owner;
GRANT SELECT(post_id,board,thread_id,actor_hash,request_at,automatic_identity,registration_xid),
    INSERT(post_id,board,thread_id,actor_hash,request_at,automatic_identity,registration_xid),UPDATE(post_id)
    ON post_secrets.posting_history TO board_thread_transfer_owner;
GRANT SELECT(post_id,token_hash,password_proof),INSERT(post_id,token_hash,password_proof),UPDATE(post_id)
    ON post_secrets.anonymous_posts TO board_thread_transfer_owner;
GRANT SELECT(token_hash,expires_at),UPDATE(token_hash)
    ON post_secrets.anonymous_sessions TO board_thread_transfer_owner;
GRANT SELECT(post_id) ON post_secrets.poster_contexts TO board_thread_transfer_owner;
GRANT CREATE ON SCHEMA content,post_secrets,staff_identity TO board_thread_transfer_owner;
SET ROLE board_thread_transfer_owner;

CREATE TABLE post_secrets.thread_transfer_gate (
    singleton boolean PRIMARY KEY CHECK(singleton)
);
INSERT INTO post_secrets.thread_transfer_gate VALUES(true);
-- Deliberately NO external FKs: session revocation, source deletion and operator
-- account deletion must not acquire this gate or remove reconciliation evidence.
CREATE TABLE post_secrets.thread_transfer_intents (
    ticket_hash bytea PRIMARY KEY CHECK(octet_length(ticket_hash)=32),
    request_id uuid NOT NULL UNIQUE DEFAULT gen_random_uuid(),
    account_id bigint NOT NULL,
    session_hash bytea NOT NULL CHECK(octet_length(session_hash)=32),
    csrf_hash bytea NOT NULL CHECK(octet_length(csrf_hash)=32),
    credential_id bytea NOT NULL,
    idle_seconds integer NOT NULL CHECK(idle_seconds BETWEEN 60 AND 3600),
    source_board text NOT NULL,source_thread bigint NOT NULL,
    destination_board text NOT NULL,destination_thread bigint NOT NULL,
    source_fingerprint bytea NOT NULL CHECK(octet_length(source_fingerprint)=32),
    formatter_version smallint NOT NULL DEFAULT 1 CHECK(formatter_version=1),
    post_count integer NOT NULL CHECK(post_count BETWEEN 1 AND 1001),
    reserved_bytes bigint NOT NULL DEFAULT 64326144 CHECK(reserved_bytes=64326144),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    sealed boolean NOT NULL DEFAULT false,
    CHECK(source_board<>destination_board AND source_thread>0 AND destination_thread>0),
    CHECK(expires_at=created_at+interval '60 seconds')
);
CREATE TABLE post_secrets.thread_transfer_map (
    ticket_hash bytea NOT NULL REFERENCES post_secrets.thread_transfer_intents ON DELETE CASCADE,
    ordinal integer NOT NULL CHECK(ordinal BETWEEN 1 AND 1001),
    source_post_id bigint NOT NULL CHECK(source_post_id>0),
    destination_post_id bigint NOT NULL UNIQUE CHECK(destination_post_id>0),
    source_hash bytea NOT NULL CHECK(octet_length(source_hash)=32),
    authorized boolean NOT NULL,comment_format smallint NOT NULL,
    wordfiltered boolean NOT NULL,
    PRIMARY KEY(ticket_hash,ordinal),UNIQUE(ticket_hash,source_post_id)
);
CREATE TABLE post_secrets.thread_transfer_sealed_rows (
    ticket_hash bytea NOT NULL,ordinal integer NOT NULL,
    comment text NOT NULL,saved_presentation bytea NOT NULL,saved_presentation_search text NOT NULL,
    PRIMARY KEY(ticket_hash,ordinal),
    FOREIGN KEY(ticket_hash,ordinal) REFERENCES post_secrets.thread_transfer_map ON DELETE CASCADE
);
-- Only the consumer creates this table's rows, after all sealed-source checks.
-- A persistent prepared map never permits an ordinary trigger to be bypassed.
CREATE TABLE post_secrets.thread_transfer_active (
    transaction_id xid8 NOT NULL,
    destination_post_id bigint PRIMARY KEY,
    board text NOT NULL,thread_id bigint NOT NULL,
    ticket_hash bytea NOT NULL,
    UNIQUE(transaction_id,destination_post_id)
);
CREATE TABLE post_secrets.thread_transfer_results (
    request_id uuid PRIMARY KEY,account_id bigint NOT NULL,
    source_board text NOT NULL,source_thread bigint NOT NULL,
    destination_board text NOT NULL,destination_thread bigint NOT NULL,
    post_count integer NOT NULL CHECK(post_count BETWEEN 1 AND 1001),
    audit_id bigint NOT NULL,committed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE(source_board,source_thread)
);
CREATE TABLE post_secrets.thread_transfer_result_posts (
    request_id uuid NOT NULL,ordinal integer NOT NULL,
    source_post_id bigint NOT NULL,destination_post_id bigint NOT NULL,
    PRIMARY KEY(request_id,ordinal)
);
REVOKE ALL ON post_secrets.thread_transfer_gate,post_secrets.thread_transfer_intents,
    post_secrets.thread_transfer_map,post_secrets.thread_transfer_sealed_rows,
    post_secrets.thread_transfer_active,post_secrets.thread_transfer_results,
    post_secrets.thread_transfer_result_posts FROM PUBLIC;

CREATE FUNCTION post_secrets.transfer_invoker() RETURNS text
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
    SELECT CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END
$$;

CREATE FUNCTION post_secrets.transfer_gate() RETURNS void
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Transfer requires Read Committed.' USING ERRCODE='P131I';
    END IF;
    IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Transfer is unavailable.' USING ERRCODE='P131R';
    END IF;
    PERFORM singleton FROM post_secrets.thread_transfer_gate WHERE singleton FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer is unavailable.' USING ERRCODE='P131R'; END IF;
END $$;

CREATE FUNCTION post_secrets.transfer_authority(p_session bytea,p_csrf bytea,p_idle integer,
    p_source text,p_destination text,p_recent boolean,p_lock boolean,p_credential bytea DEFAULT NULL) RETURNS bigint
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_account bigint; v_credential bytea; v_recent boolean;
BEGIN
    IF p_session IS NULL OR octet_length(p_session)<>32 OR p_csrf IS NULL OR octet_length(p_csrf)<>32
        OR p_idle IS NULL OR p_idle NOT BETWEEN 60 AND 3600 THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A';
    END IF;
    SELECT s.account_id,s.credential_id INTO v_account,v_credential
        FROM staff_identity.sessions s WHERE s.token_hash=p_session;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    IF p_credential IS NOT NULL AND p_credential IS DISTINCT FROM v_credential THEN
        RAISE EXCEPTION 'Transfer credential changed.' USING ERRCODE='P131A';
    END IF;
    IF p_lock THEN
        PERFORM a.id FROM staff_identity.accounts a WHERE a.id=v_account FOR SHARE NOWAIT;
        PERFORM s.token_hash FROM staff_identity.sessions s WHERE s.token_hash=p_session FOR SHARE NOWAIT;
        PERFORM c.id FROM staff_identity.credentials c WHERE c.id=v_credential FOR KEY SHARE NOWAIT;
    END IF;
    SELECT s.authenticated_at>clock_timestamp()-interval '10 minutes' INTO v_recent
    FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id
    JOIN staff_identity.credentials c ON c.id=s.credential_id AND c.account_id=a.id
    WHERE s.token_hash=p_session AND s.account_id=v_account AND s.credential_id=v_credential
        AND s.csrf_hash=p_csrf AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>p_idle)
        AND a.revoked_at IS NULL AND a.role IN('moderator','manager','admin')
        AND staff_identity.has_board_access(a.id,p_source)
        AND staff_identity.has_board_access(a.id,p_destination);
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    IF p_recent AND NOT v_recent THEN RAISE EXCEPTION 'Recent authentication is required.' USING ERRCODE='P131Q'; END IF;
    RETURN v_account;
END $$;

CREATE FUNCTION post_secrets.transfer_lock_source(p_source text,p_thread bigint,p_destination text)
RETURNS integer LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_count integer; v_bytes bigint; v_all_bytes bigint;
BEGIN
    IF p_source IS NULL OR p_destination IS NULL OR p_source !~ '^[a-z0-9]{1,10}$'
        OR p_destination !~ '^[a-z0-9]{1,10}$' OR p_source=p_destination OR p_source='b'
        OR p_destination IN('f','j') OR p_thread IS NULL OR p_thread<=0 THEN
        RAISE EXCEPTION 'Invalid transfer target.' USING ERRCODE='P131I';
    END IF;
    PERFORM b.slug FROM content.boards b WHERE b.slug IN(p_source,p_destination)
        ORDER BY b.slug FOR UPDATE NOWAIT;
    IF (SELECT count(*) FROM content.boards b WHERE b.slug IN(p_source,p_destination)
        AND NOT b.staff_only AND NOT b.upload_board)<>2 THEN
        RAISE EXCEPTION 'Transfer board is unavailable.' USING ERRCODE='P131N';
    END IF;
    PERFORM t.id FROM content.threads t WHERE t.board=p_source AND t.id=p_thread
        AND NOT t.deleted AND NOT t.content_erased AND NOT t.closed AND t.archived_at IS NULL FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer source is unavailable.' USING ERRCODE='P131N'; END IF;
    -- Retained sticky tombstones can outgrow the live window. Bound all work,
    -- including structural-media inspection, rather than sweeping that history.
    IF (SELECT count(*) FROM (SELECT 1 FROM content.posts p WHERE p.board=p_source
        AND p.thread_id=p_thread LIMIT 1002) bounded)>1001 THEN
        RAISE EXCEPTION 'Transfer source exceeds its bounded window.' USING ERRCODE='P131L';
    END IF;
    PERFORM p.id FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread ORDER BY p.id FOR UPDATE NOWAIT;
    IF NOT EXISTS(SELECT 1 FROM content.posts p WHERE p.board=p_source AND p.id=p_thread
        AND p.thread_id=p_thread AND NOT p.deleted AND NOT p.content_erased) THEN
        RAISE EXCEPTION 'Transfer source is unavailable.' USING ERRCODE='P131N';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p JOIN content.post_media m ON m.post_id=p.id
        WHERE p.board=p_source AND p.thread_id=p_thread) THEN
        RAISE EXCEPTION 'Only media-free threads can be transferred.' USING ERRCODE='P131F';
    END IF;
    SELECT count(*),coalesce(sum(octet_length(p.comment)::bigint+coalesce(octet_length(p.wordfilter_payload),0)
        +coalesce(octet_length(p.wordfilter_search),0)+coalesce(octet_length(p.saved_presentation),0)
        +coalesce(octet_length(p.saved_presentation_search),0)),0),
        coalesce(sum(octet_length(p.comment)::bigint+coalesce(octet_length(p.wordfilter_payload),0)
        +coalesce(octet_length(p.wordfilter_search),0)+coalesce(octet_length(p.saved_presentation),0)
        +coalesce(octet_length(p.saved_presentation_search),0)+octet_length(p.name)+octet_length(p.subject)+2048),0)
        INTO v_count,v_bytes,v_all_bytes
    FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted;
    IF v_count NOT BETWEEN 1 AND 1001 OR v_bytes>64064000 OR v_all_bytes>67108864 THEN
        RAISE EXCEPTION 'Transfer exceeds its capacity.' USING ERRCODE='P131L';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted
        AND (p.content_erased OR NOT isfinite(p.created_at)
            OR NOT (p.comment_format BETWEEN 104 AND 111 OR p.comment_format BETWEEN 120 AND 127)
            OR p.drawing_time_seconds IS NOT NULL OR p.drawing_source_post_id IS NOT NULL
            OR p.dice_result IS NOT NULL OR p.fortune_text IS NOT NULL OR p.fortune_color IS NOT NULL
            OR (p.saved_presentation IS NOT NULL AND p.wordfilter_payload IS NOT NULL))) THEN
        RAISE EXCEPTION 'Source presentation cannot be transferred.' USING ERRCODE='P131F';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p JOIN content.boards b ON b.slug=p.board
        WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted
        AND b.country_flags AND p.board_flag IS NULL AND (p.country IS NULL OR p.country_name IS NULL)) THEN
        RAISE EXCEPTION 'Source country evidence is unavailable.' USING ERRCODE='P131F';
    END IF;
    RETURN v_count;
END $$;

-- Explicit fields, not xmin or a reader projection. Bytea and timestamps have
-- canonical spellings independent of the caller's connection settings.
CREATE FUNCTION post_secrets.transfer_post_hash(p_id bigint) RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 SELECT sha256(convert_to(jsonb_build_array(p.id,p.board,p.thread_id,p.name,p.trip,p.subject,p.comment,
    p.created_at,p.deleted,p.content_erased,p.capcode,p.poster_id,p.json_op_poster_id,p.country,p.country_name,
    p.board_flag,p.board_flag_type,p.flag_name,p.image_spoiler,p.file_deleted,p.comment_format,
    p.staff_authorized_limits,encode(p.wordfilter_payload,'hex'),p.wordfilter_search,p.drawing_time_seconds,
    p.drawing_source_post_id,p.dice_result,p.fortune_text,p.fortune_color,
    encode(p.saved_presentation,'hex'),p.saved_presentation_search)::text,'UTF8'))
 FROM content.posts p WHERE p.id=p_id
$$;
CREATE FUNCTION post_secrets.transfer_source_hash(p_source text,p_thread bigint,p_destination text) RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 SELECT sha256(convert_to(jsonb_build_array('text-transfer-v1',p_source,p_thread,p_destination,1,
    (SELECT jsonb_build_array(t.id,t.board,t.created_at,t.bumped_at,t.modified_at,t.http_modified_at,
        t.reply_count,t.sticky,t.sticky_rank,t.closed,t.permasage,t.permaage,t.undead,t.archived_at,
        t.archive_expires_at,t.deleted,t.content_erased) FROM content.threads t WHERE t.board=p_source AND t.id=p_thread),
    (SELECT jsonb_agg(jsonb_build_array(b.slug,b.staff_only,b.upload_board,b.country_flags,b.reply_limit,b.thread_limit)
        ORDER BY b.slug) FROM content.boards b WHERE b.slug IN(p_source,p_destination)),
    (SELECT jsonb_agg(jsonb_build_array(p.id,encode(post_secrets.transfer_post_hash(p.id),'hex'))
        ORDER BY (p.id=p_thread) DESC,p.id) FROM content.posts p
        WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted))::text,'UTF8'))
$$;

CREATE FUNCTION content.is_thread_transfer_post(p_id bigint,p_board text,p_thread bigint) RETURNS boolean
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT post_secrets.transfer_invoker()='board_staff' AND EXISTS(
    SELECT 1 FROM post_secrets.thread_transfer_active a JOIN post_secrets.thread_transfer_intents i
      ON i.ticket_hash=a.ticket_hash
    WHERE a.transaction_id=pg_current_xact_id() AND a.destination_post_id=p_id
      AND a.board=p_board AND a.thread_id=p_thread AND i.sealed)
$$;
CREATE FUNCTION post_secrets.reject_orphan_thread_transfer() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM post_secrets.thread_transfer_active WHERE destination_post_id=NEW.destination_post_id) THEN
        RAISE EXCEPTION 'Incomplete transfer.' USING ERRCODE='P131I';
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER reject_orphan_thread_transfer AFTER INSERT ON post_secrets.thread_transfer_active
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION post_secrets.reject_orphan_thread_transfer();

CREATE FUNCTION content.guard_saved_presentation() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.saved_presentation IS NOT NULL AND NOT content.is_thread_transfer_post(NEW.id,NEW.board,NEW.thread_id) THEN
            RAISE EXCEPTION 'Frozen presentation requires sealed transfer authority.' USING ERRCODE='P131A';
        END IF;
    ELSIF OLD.saved_presentation IS NOT NULL OR NEW.saved_presentation IS NOT NULL THEN
        IF ROW(NEW.saved_presentation,NEW.saved_presentation_search,NEW.comment,NEW.comment_format,
            NEW.staff_authorized_limits,NEW.wordfilter_payload,NEW.wordfilter_search)
            IS DISTINCT FROM ROW(OLD.saved_presentation,OLD.saved_presentation_search,OLD.comment,OLD.comment_format,
            OLD.staff_authorized_limits,OLD.wordfilter_payload,OLD.wordfilter_search) THEN
            RAISE EXCEPTION 'Frozen presentation is immutable.' USING ERRCODE='P131I';
        END IF;
    END IF;
    -- Canonical z_guard_post_erasure runs later and clears the entire payload.
    RETURN NEW;
END $$;

CREATE FUNCTION staff_identity.prepare_text_thread_transfer(p_ticket bytea,p_session bytea,p_csrf bytea,
    p_idle integer,p_source text,p_thread bigint,p_destination text,p_mode smallint)
RETURNS TABLE(request_id text,expires_at timestamptz,source_fingerprint bytea,source_board text,
    source_thread bigint,destination_board text,destination_thread bigint,ordinal integer,
    source_post_id bigint,destination_post_id bigint,comment text,comment_format smallint,
    staff_authorized_limits boolean,wordfilter_payload bytea,saved_presentation bytea)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_account bigint; v_count integer; v_destination bigint; v_created timestamptz;
    v_hash bytea; v_ordinal integer:=0; v_new bigint; r record;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_auth' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    IF p_ticket IS NULL OR octet_length(p_ticket)<>32 OR p_mode IS DISTINCT FROM 1::smallint THEN
        RAISE EXCEPTION 'Invalid transfer request.' USING ERRCODE='P131I'; END IF;
    PERFORM post_secrets.transfer_gate();
    DELETE FROM post_secrets.thread_transfer_intents i WHERE i.ticket_hash IN(
        SELECT expired.ticket_hash FROM post_secrets.thread_transfer_intents expired
        WHERE expired.expires_at<=clock_timestamp() ORDER BY expired.expires_at,expired.ticket_hash
        LIMIT 2 FOR UPDATE SKIP LOCKED);
    v_account:=post_secrets.transfer_authority(p_session,p_csrf,p_idle,p_source,p_destination,true,true);
    IF EXISTS(SELECT 1 FROM post_secrets.thread_transfer_intents i WHERE i.account_id=v_account)
        OR (SELECT count(*) FROM post_secrets.thread_transfer_intents)>=2
        OR (SELECT coalesce(sum(reserved_bytes),0) FROM post_secrets.thread_transfer_intents)+64326144>134217728 THEN
        RAISE EXCEPTION 'Transfer capacity is occupied.' USING ERRCODE='P131L';
    END IF;
    v_count:=post_secrets.transfer_lock_source(p_source,p_thread,p_destination);
    v_hash:=post_secrets.transfer_source_hash(p_source,p_thread,p_destination);
    v_destination:=nextval('content.post_number'); v_created:=clock_timestamp();
    INSERT INTO post_secrets.thread_transfer_intents(ticket_hash,account_id,session_hash,csrf_hash,
        credential_id,idle_seconds,source_board,source_thread,destination_board,destination_thread,
        source_fingerprint,post_count,created_at,expires_at)
    SELECT p_ticket,v_account,p_session,p_csrf,s.credential_id,p_idle,p_source,p_thread,p_destination,
        v_destination,v_hash,v_count,v_created,v_created+interval '60 seconds'
        FROM staff_identity.sessions s WHERE s.token_hash=p_session;
    FOR r IN SELECT p.id,p.staff_authorized_limits,p.comment_format,p.wordfilter_payload,p.saved_presentation
        FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted
        ORDER BY (p.id=p_thread) DESC,p.id LOOP
        v_ordinal:=v_ordinal+1;
        v_new:=CASE WHEN v_ordinal=1 THEN v_destination ELSE nextval('content.post_number') END;
        INSERT INTO post_secrets.thread_transfer_map(ticket_hash,ordinal,source_post_id,destination_post_id,
            source_hash,authorized,comment_format,wordfiltered)
        VALUES(p_ticket,v_ordinal,r.id,v_new,post_secrets.transfer_post_hash(r.id),r.staff_authorized_limits,
            r.comment_format,r.wordfilter_payload IS NOT NULL OR
                CASE WHEN r.saved_presentation IS NULL THEN false ELSE get_byte(r.saved_presentation,4)=1 END);
    END LOOP;
    PERFORM post_secrets.transfer_authority(p_session,p_csrf,p_idle,p_source,p_destination,true,false);
    RETURN QUERY SELECT i.request_id::text,i.expires_at,i.source_fingerprint,i.source_board,i.source_thread,
        i.destination_board,i.destination_thread,m.ordinal,m.source_post_id,m.destination_post_id,
        p.comment,p.comment_format,p.staff_authorized_limits,p.wordfilter_payload,p.saved_presentation
    FROM post_secrets.thread_transfer_intents i JOIN post_secrets.thread_transfer_map m ON m.ticket_hash=i.ticket_hash
    JOIN content.posts p ON p.id=m.source_post_id WHERE i.ticket_hash=p_ticket ORDER BY m.ordinal;
END $$;

CREATE FUNCTION staff_identity.seal_text_thread_transfer(p_ticket bytea,p_session bytea,p_csrf bytea,
    p_fingerprint bytea,p_version smallint,p_old bigint[],p_new bigint[],p_comment text[],p_fp bytea[],p_search text[])
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE i post_secrets.thread_transfer_intents%ROWTYPE; v_account bigint; n integer; total bigint:=0; m record;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_auth' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM post_secrets.transfer_gate();
    SELECT * INTO i FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    IF p_session IS DISTINCT FROM i.session_hash OR p_csrf IS DISTINCT FROM i.csrf_hash THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    v_account:=post_secrets.transfer_authority(p_session,p_csrf,i.idle_seconds,i.source_board,i.destination_board,true,true,i.credential_id);
    PERFORM ticket_hash FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket FOR UPDATE NOWAIT;
    IF v_account<>i.account_id OR i.expires_at<=clock_timestamp() THEN
        RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    IF i.sealed OR p_fingerprint IS DISTINCT FROM i.source_fingerprint OR p_version IS DISTINCT FROM i.formatter_version
        OR cardinality(p_old) IS DISTINCT FROM i.post_count OR cardinality(p_new) IS DISTINCT FROM i.post_count
        OR cardinality(p_comment) IS DISTINCT FROM i.post_count OR cardinality(p_fp) IS DISTINCT FROM i.post_count
        OR cardinality(p_search) IS DISTINCT FROM i.post_count
        OR array_ndims(p_old)<>1 OR array_ndims(p_new)<>1 OR array_ndims(p_comment)<>1
        OR array_ndims(p_fp)<>1 OR array_ndims(p_search)<>1
        OR array_lower(p_old,1)<>1 OR array_lower(p_new,1)<>1 OR array_lower(p_comment,1)<>1
        OR array_lower(p_fp,1)<>1 OR array_lower(p_search,1)<>1 THEN
        RAISE EXCEPTION 'Invalid sealed transfer.' USING ERRCODE='P131I'; END IF;
    FOR m IN SELECT * FROM post_secrets.thread_transfer_map WHERE ticket_hash=p_ticket ORDER BY ordinal LOOP
        n:=m.ordinal;
        IF p_old[n] IS DISTINCT FROM m.source_post_id OR p_new[n] IS DISTINCT FROM m.destination_post_id
            OR p_comment[n] IS NULL OR p_fp[n] IS NULL OR p_search[n] IS NULL THEN
            RAISE EXCEPTION 'Invalid sealed transfer.' USING ERRCODE='P131I'; END IF;
        IF octet_length(p_fp[n]) NOT BETWEEN 9 AND (CASE WHEN m.authorized THEN 524288 ELSE 131072 END)
            OR substring(p_fp[n] FROM 1 FOR 4)<>decode(CASE WHEN m.authorized THEN '46503032' ELSE '46503031' END,'hex')
            OR octet_length(p_comment[n])>(CASE WHEN m.authorized THEN 2097152 ELSE 524288 END)
            OR octet_length(p_search[n])>(CASE WHEN m.authorized THEN 524288 ELSE 131072 END) THEN
            RAISE EXCEPTION 'Sealed presentation exceeds its bounds.' USING ERRCODE='P131F'; END IF;
        IF get_byte(p_fp[n],4)<>m.wordfiltered::integer THEN
            RAISE EXCEPTION 'Invalid sealed provenance.' USING ERRCODE='P131F'; END IF;
        total:=total+octet_length(p_comment[n])+octet_length(p_fp[n])+octet_length(p_search[n]);
        IF total>64064000 THEN RAISE EXCEPTION 'Transfer exceeds its capacity.' USING ERRCODE='P131L'; END IF;
        INSERT INTO post_secrets.thread_transfer_sealed_rows VALUES(p_ticket,n,p_comment[n],p_fp[n],p_search[n]);
    END LOOP;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    PERFORM post_secrets.transfer_authority(p_session,p_csrf,i.idle_seconds,i.source_board,i.destination_board,true,false,i.credential_id);
    UPDATE post_secrets.thread_transfer_intents SET sealed=true WHERE ticket_hash=p_ticket;
END $$;

CREATE FUNCTION staff_identity.discard_text_thread_transfer(p_ticket bytea,p_session bytea,p_csrf bytea)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE i post_secrets.thread_transfer_intents%ROWTYPE;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_auth' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM post_secrets.transfer_gate();
    SELECT * INTO i FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    IF NOT FOUND THEN RETURN; END IF;
    IF i.session_hash IS DISTINCT FROM p_session OR i.csrf_hash IS DISTINCT FROM p_csrf
        OR post_secrets.transfer_authority(p_session,p_csrf,i.idle_seconds,i.source_board,i.destination_board,false,true,i.credential_id)<>i.account_id THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM ticket_hash FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket FOR UPDATE NOWAIT;
    DELETE FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
END $$;

CREATE FUNCTION content.consume_text_thread_transfer(p_ticket bytea)
RETURNS TABLE(request_id text,source_board text,source_thread bigint,destination_board text,
    destination_thread bigint,post_count bigint,audit_id bigint,source_post_id bigint,destination_post_id bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE i post_secrets.thread_transfer_intents%ROWTYPE; v_count integer; v_audit bigint; v_now timestamptz;
    v_inserted integer; v_anonymous_at bigint;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_staff' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM post_secrets.transfer_gate();
    SELECT * INTO i FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    IF post_secrets.transfer_authority(i.session_hash,i.csrf_hash,i.idle_seconds,
        i.source_board,i.destination_board,true,true,i.credential_id)<>i.account_id THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM ticket_hash FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket FOR UPDATE NOWAIT;
    IF NOT i.sealed THEN RAISE EXCEPTION 'Transfer is not sealed.' USING ERRCODE='P131I'; END IF;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    v_count:=post_secrets.transfer_lock_source(i.source_board,i.source_thread,i.destination_board);
    IF v_count<>i.post_count OR post_secrets.transfer_source_hash(i.source_board,i.source_thread,i.destination_board)
        IS DISTINCT FROM i.source_fingerprint OR EXISTS(
            SELECT 1 FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket
            AND post_secrets.transfer_post_hash(m.source_post_id) IS DISTINCT FROM m.source_hash) THEN
        RAISE EXCEPTION 'Transfer source changed.' USING ERRCODE='P131C'; END IF;
    IF (SELECT count(*) FROM post_secrets.thread_transfer_map WHERE ticket_hash=p_ticket)<>v_count
        OR (SELECT count(*) FROM post_secrets.thread_transfer_sealed_rows WHERE ticket_hash=p_ticket)<>v_count
        OR EXISTS(SELECT 1 FROM post_secrets.thread_transfer_map m JOIN content.posts p ON p.id=m.destination_post_id WHERE m.ticket_hash=p_ticket)
        OR EXISTS(SELECT 1 FROM content.threads t WHERE t.id=i.destination_thread) THEN
        RAISE EXCEPTION 'Transfer destination changed.' USING ERRCODE='P131C'; END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM content.threads t WHERE t.board=i.destination_board AND NOT t.deleted
        AND t.archived_at IS NULL AND NOT t.sticky AND NOT t.undead LIMIT 1001) occupied)>=
        (SELECT b.thread_limit FROM content.boards b WHERE b.slug=i.destination_board)
        OR (SELECT count(*) FROM (SELECT 1 FROM post_secrets.posting_history h WHERE h.board=i.destination_board LIMIT 100001) occupied)
          +(SELECT count(*) FROM post_secrets.posting_history h JOIN post_secrets.thread_transfer_map m
            ON m.source_post_id=h.post_id WHERE m.ticket_hash=p_ticket)>100000 THEN
        RAISE EXCEPTION 'Transfer destination capacity is exhausted.' USING ERRCODE='P131L'; END IF;
    -- Session before membership; NOWAIT avoids reverse-order expiry cleanup.
    PERFORM s.token_hash FROM post_secrets.anonymous_sessions s WHERE EXISTS(
        SELECT 1 FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.source_post_id=a.post_id
        WHERE m.ticket_hash=p_ticket AND a.token_hash=s.token_hash) ORDER BY s.token_hash FOR UPDATE NOWAIT;
    PERFORM d.post_id FROM post_secrets.deletion d JOIN post_secrets.thread_transfer_map m ON m.source_post_id=d.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY d.post_id FOR SHARE OF d NOWAIT;
    PERFORM o.thread_id FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread FOR SHARE NOWAIT;
    PERFORM r.post_id FROM post_secrets.op_replies r JOIN post_secrets.thread_transfer_map m ON m.source_post_id=r.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY r.post_id FOR SHARE OF r NOWAIT;
    PERFORM h.post_id FROM post_secrets.posting_history h JOIN post_secrets.thread_transfer_map m ON m.source_post_id=h.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY h.post_id FOR SHARE OF h NOWAIT;
    PERFORM a.post_id FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.source_post_id=a.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY a.post_id FOR SHARE OF a NOWAIT;
    INSERT INTO post_secrets.thread_transfer_active
        SELECT pg_current_xact_id(),m.destination_post_id,i.destination_board,i.destination_thread,p_ticket
        FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket;
    v_now:=clock_timestamp();
    INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,http_modified_at,reply_count,
        sticky,sticky_rank,closed,permasage,permaage,undead,archived_at,archive_expires_at,deleted,content_erased)
    SELECT i.destination_thread,i.destination_board,p.created_at,v_now,p.created_at,v_now,v_count-1,
        false,0,false,false,false,false,NULL,NULL,false,false
    FROM content.posts p WHERE p.id=i.source_thread AND p.board=i.source_board;
    INSERT INTO content.posts(id,board,thread_id,name,trip,subject,comment,created_at,capcode,poster_id,json_op_poster_id,
        country,country_name,board_flag,board_flag_type,flag_name,image_spoiler,file_deleted,comment_format,
        staff_authorized_limits,saved_presentation,saved_presentation_search)
    SELECT m.destination_post_id,i.destination_board,i.destination_thread,p.name,p.trip,p.subject,s.comment,p.created_at,
        p.capcode,NULL,NULL,CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country ELSE 'XX' END,
        CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country_name ELSE 'Unknown' END,
        NULL,'pol',NULL,false,p.file_deleted,m.comment_format,m.authorized,s.saved_presentation,s.saved_presentation_search
    FROM post_secrets.thread_transfer_map m JOIN post_secrets.thread_transfer_sealed_rows s
        ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal JOIN content.posts p ON p.id=m.source_post_id
    JOIN content.boards b ON b.slug=i.source_board WHERE m.ticket_hash=p_ticket ORDER BY m.ordinal;
    GET DIAGNOSTICS v_inserted=ROW_COUNT;
    IF v_inserted<>v_count OR EXISTS(
        SELECT 1 FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.thread_transfer_sealed_rows s ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal
        JOIN content.posts p ON p.id=m.source_post_id JOIN content.boards b ON b.slug=i.source_board
        LEFT JOIN content.posts d ON d.id=m.destination_post_id WHERE m.ticket_hash=p_ticket AND (
            d.id IS NULL OR ROW(d.board,d.thread_id,d.name,d.trip,d.subject,d.comment,d.created_at,d.capcode,
                d.poster_id,d.json_op_poster_id,d.country,d.country_name,d.board_flag,d.board_flag_type,d.flag_name,
                d.image_spoiler,d.file_deleted,d.comment_format,d.staff_authorized_limits,d.saved_presentation,
                d.saved_presentation_search,d.wordfilter_payload,d.wordfilter_search,d.drawing_time_seconds,
                d.drawing_source_post_id,d.dice_result,d.fortune_text,d.fortune_color,d.deleted,d.content_erased)
              IS DISTINCT FROM ROW(i.destination_board,i.destination_thread,p.name,p.trip,p.subject,s.comment,
                p.created_at,p.capcode,NULL::text,NULL::text,
                CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country ELSE 'XX' END,
                CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country_name ELSE 'Unknown' END,
                NULL::text,'pol'::text,NULL::text,false,p.file_deleted,m.comment_format,m.authorized,
                s.saved_presentation,s.saved_presentation_search,NULL::bytea,NULL::text,NULL::integer,
                NULL::bigint,NULL::text,NULL::text,NULL::text,false,false))) THEN
        RAISE EXCEPTION 'Transfer payload verification failed.' USING ERRCODE='P131R';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM content.threads t JOIN content.posts p ON p.id=i.source_thread
        WHERE t.id=i.destination_thread AND ROW(t.board,t.created_at,t.bumped_at,t.modified_at,t.http_modified_at,
            t.reply_count,t.sticky,t.sticky_rank,t.closed,t.permasage,t.permaage,t.undead,t.archived_at,
            t.archive_expires_at,t.deleted,t.content_erased)
          IS NOT DISTINCT FROM ROW(i.destination_board,p.created_at,v_now,p.created_at,v_now,v_count-1,
            false,0::smallint,false,false,false,false,NULL::timestamptz,NULL::timestamptz,false,false)) THEN
        RAISE EXCEPTION 'Transfer thread verification failed.' USING ERRCODE='P131R';
    END IF;
    INSERT INTO post_secrets.deletion(post_id,password_hash)
        SELECT m.destination_post_id,d.password_hash FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket;
    INSERT INTO post_secrets.op_peers(thread_id,peer)
        SELECT i.destination_thread,o.peer FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread;
    INSERT INTO post_secrets.op_replies(post_id,thread_id)
        SELECT m.destination_post_id,i.destination_thread FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.op_replies r ON r.post_id=m.source_post_id AND r.thread_id=i.source_thread
        WHERE m.ticket_hash=p_ticket;
    INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at,automatic_identity,registration_xid)
        SELECT m.destination_post_id,i.destination_board,i.destination_thread,h.actor_hash,h.request_at,h.automatic_identity,
            CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid END
        FROM post_secrets.thread_transfer_map m JOIN post_secrets.posting_history h ON h.post_id=m.source_post_id
            AND h.board=i.source_board AND h.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket;
    v_anonymous_at:=extract(epoch FROM clock_timestamp())::bigint;
    INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
        SELECT m.destination_post_id,a.token_hash,a.password_proof FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.anonymous_posts a ON a.post_id=m.source_post_id
        JOIN post_secrets.anonymous_sessions s ON s.token_hash=a.token_hash AND s.expires_at>v_anonymous_at
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
        WHERE m.ticket_hash=p_ticket;
    -- Compare complete private row sets, not only counts. These sets and all
    -- hashes/peers/UUIDs remain inside SQL; errors contain no evidence values.
    IF EXISTS(
        WITH expected(kind,id,payload) AS (
            SELECT 'deletion',m.destination_post_id,jsonb_build_array(d.password_hash)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.deletion d ON d.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'peer',i.destination_thread,jsonb_build_array(o.peer)
            FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread
            UNION ALL SELECT 'reply',m.destination_post_id,jsonb_build_array(i.destination_thread)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.op_replies r ON r.post_id=m.source_post_id
                AND r.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'history',m.destination_post_id,jsonb_build_array(i.destination_board,i.destination_thread,
                encode(h.actor_hash,'hex'),h.request_at,h.automatic_identity,
                CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid::text END)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.posting_history h ON h.post_id=m.source_post_id
                AND h.board=i.source_board AND h.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'anonymous',m.destination_post_id,jsonb_build_array(encode(a.token_hash,'hex'),encode(a.password_proof,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.anonymous_posts a ON a.post_id=m.source_post_id
            JOIN post_secrets.anonymous_sessions session ON session.token_hash=a.token_hash AND session.expires_at>v_anonymous_at
            JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
            WHERE m.ticket_hash=p_ticket
        ), actual(kind,id,payload) AS (
            SELECT 'deletion',d.post_id,jsonb_build_array(d.password_hash) FROM post_secrets.deletion d
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=d.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'peer',o.thread_id,jsonb_build_array(o.peer) FROM post_secrets.op_peers o WHERE o.thread_id=i.destination_thread
            UNION ALL SELECT 'reply',r.post_id,jsonb_build_array(r.thread_id) FROM post_secrets.op_replies r
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=r.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'history',h.post_id,jsonb_build_array(h.board,h.thread_id,encode(h.actor_hash,'hex'),
                h.request_at,h.automatic_identity,h.registration_xid::text) FROM post_secrets.posting_history h
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=h.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'anonymous',a.post_id,jsonb_build_array(encode(a.token_hash,'hex'),encode(a.password_proof,'hex'))
                FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=a.post_id WHERE m.ticket_hash=p_ticket
        ) SELECT 1 FROM ((SELECT * FROM expected EXCEPT ALL SELECT * FROM actual)
            UNION ALL (SELECT * FROM actual EXCEPT ALL SELECT * FROM expected)) different
    ) OR EXISTS(SELECT 1 FROM post_secrets.poster_contexts c JOIN post_secrets.thread_transfer_map m
        ON m.destination_post_id=c.post_id WHERE m.ticket_hash=p_ticket) THEN
        RAISE EXCEPTION 'Transfer evidence verification failed.' USING ERRCODE='P131R';
    END IF;
    UPDATE content.threads SET deleted=true WHERE board=i.source_board AND id=i.source_thread;
    INSERT INTO content.moderation_audit(account_id,board,target_id,action)
        VALUES(i.account_id,i.source_board,i.source_thread,'text-thread-transfer') RETURNING id INTO v_audit;
    INSERT INTO post_secrets.thread_transfer_results(request_id,account_id,source_board,source_thread,
        destination_board,destination_thread,post_count,audit_id)
        VALUES(i.request_id,i.account_id,i.source_board,i.source_thread,i.destination_board,i.destination_thread,v_count,v_audit);
    INSERT INTO post_secrets.thread_transfer_result_posts
        SELECT i.request_id,m.ordinal,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    PERFORM post_secrets.transfer_authority(i.session_hash,i.csrf_hash,i.idle_seconds,
        i.source_board,i.destination_board,true,false,i.credential_id);
    DELETE FROM post_secrets.thread_transfer_active WHERE ticket_hash=p_ticket AND transaction_id=pg_current_xact_id();
    DELETE FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    RETURN QUERY SELECT r.request_id::text,r.source_board,r.source_thread,r.destination_board,r.destination_thread,
        r.post_count::bigint,r.audit_id,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_results r JOIN post_secrets.thread_transfer_result_posts m ON m.request_id=r.request_id
        WHERE r.request_id=i.request_id ORDER BY m.ordinal;
END $$;

CREATE FUNCTION staff_identity.text_thread_transfer_result(p_request text,p_session bytea,p_csrf bytea,p_idle integer,
    p_source text DEFAULT NULL,p_thread bigint DEFAULT NULL)
RETURNS TABLE(request_id text,source_board text,source_thread bigint,destination_board text,
    destination_thread bigint,post_count bigint,audit_id bigint,source_post_id bigint,destination_post_id bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE r post_secrets.thread_transfer_results%ROWTYPE; v_account bigint;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_auth' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    IF p_session IS NULL OR octet_length(p_session)<>32 OR p_csrf IS NULL OR octet_length(p_csrf)<>32
        OR p_idle IS NULL OR p_idle NOT BETWEEN 60 AND 3600 THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    -- Authenticate before receipt selection. An exact source lookup must not
    -- reveal another account's receipt through a board-scope error.
    SELECT a.id INTO v_account FROM staff_identity.sessions s
        JOIN staff_identity.accounts a ON a.id=s.account_id
        JOIN staff_identity.credentials c ON c.id=s.credential_id AND c.account_id=a.id
        WHERE s.token_hash=p_session AND s.csrf_hash=p_csrf
            AND s.expires_at>clock_timestamp()
            AND s.last_activity_at>clock_timestamp()-make_interval(secs=>p_idle)
            AND a.revoked_at IS NULL AND a.role IN('moderator','manager','admin');
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    SELECT * INTO r FROM post_secrets.thread_transfer_results x
        WHERE x.account_id=v_account AND
          ((p_request IS NOT NULL AND x.request_id::text=p_request AND p_source IS NULL AND p_thread IS NULL)
           OR (p_request IS NULL AND x.source_board=p_source AND x.source_thread=p_thread));
    IF NOT FOUND THEN RETURN; END IF;
    v_account:=post_secrets.transfer_authority(p_session,p_csrf,p_idle,r.source_board,r.destination_board,false,false);
    IF v_account<>r.account_id THEN RETURN; END IF;
    RETURN QUERY SELECT r.request_id::text,r.source_board,r.source_thread,r.destination_board,r.destination_thread,
        r.post_count::bigint,r.audit_id,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_result_posts m WHERE m.request_id=r.request_id ORDER BY m.ordinal;
END $$;

-- Functions are private by default. Only these finite role-split wrappers are callable.
REVOKE ALL ON FUNCTION post_secrets.transfer_invoker(),post_secrets.transfer_gate(),
    post_secrets.transfer_authority(bytea,bytea,integer,text,text,boolean,boolean,bytea),
    post_secrets.transfer_lock_source(text,bigint,text),post_secrets.transfer_post_hash(bigint),
    post_secrets.transfer_source_hash(text,bigint,text),post_secrets.reject_orphan_thread_transfer(),
    content.guard_saved_presentation() FROM PUBLIC,board_public,board_staff,board_auth;
REVOKE ALL ON FUNCTION content.is_thread_transfer_post(bigint,text,bigint),
    staff_identity.prepare_text_thread_transfer(bytea,bytea,bytea,integer,text,bigint,text,smallint),
    staff_identity.seal_text_thread_transfer(bytea,bytea,bytea,bytea,smallint,bigint[],bigint[],text[],bytea[],text[]),
    staff_identity.discard_text_thread_transfer(bytea,bytea,bytea),content.consume_text_thread_transfer(bytea),
    staff_identity.text_thread_transfer_result(text,bytea,bytea,integer,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.prepare_text_thread_transfer(bytea,bytea,bytea,integer,text,bigint,text,smallint),
    staff_identity.seal_text_thread_transfer(bytea,bytea,bytea,bytea,smallint,bigint[],bigint[],text[],bytea[],text[]),
    staff_identity.discard_text_thread_transfer(bytea,bytea,bytea),
    staff_identity.text_thread_transfer_result(text,bytea,bytea,integer,text,bigint) TO board_auth;
GRANT EXECUTE ON FUNCTION content.consume_text_thread_transfer(bytea) TO board_staff;
-- Predicate exposes no row or secret, and is false outside this transaction's
-- exact active handoff. Trigger WHEN expressions execute with their caller.
GRANT EXECUTE ON FUNCTION content.is_thread_transfer_post(bigint,text,bigint)
    TO board_public,board_staff,board_attachment_owner,board_staff_post_owner,board_migrator;
GRANT EXECUTE ON FUNCTION content.guard_saved_presentation() TO board_migrator;
RESET ROLE;
CREATE TRIGGER aa_guard_saved_presentation BEFORE INSERT OR UPDATE ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.guard_saved_presentation();
SET ROLE board_thread_transfer_owner;
REVOKE EXECUTE ON FUNCTION content.guard_saved_presentation() FROM board_migrator;
RESET ROLE;

-- Remaining trigger gating, canonical erasure extension and readiness below.

-- Preserve 0130 verbatim except clearing the two new canonical payload fields.
GRANT CREATE ON SCHEMA post_secrets TO board_content_erasure_owner;
SET ROLE board_content_erasure_owner;
CREATE OR REPLACE FUNCTION post_secrets.guard_post_erasure() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE parent_erased boolean; invoker text;
BEGIN
    IF TG_OP='INSERT' THEN
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content insertion requires Read Committed.' USING ERRCODE='22023';
        END IF;
        IF NEW.content_erased OR NEW.deleted THEN
            RAISE EXCEPTION 'Cannot insert deleted content.' USING ERRCODE='23514';
        END IF;
        -- Serialize even direct runtime inserts with thread erasure. Existing
        -- posting already holds this lock before thread/post/secret tuples.
        PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE;
        IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
        IF EXISTS(SELECT 1 FROM content.threads t WHERE t.board=NEW.board
            AND t.id=NEW.thread_id AND t.content_erased) THEN
            RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    -- Post numbers are durable identities, including media consumption links.
    IF NEW.id<>OLD.id THEN
        RAISE EXCEPTION 'Post identity cannot be reassigned.' USING ERRCODE='23514';
    END IF;
    IF NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id THEN
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content reassignment requires Read Committed.' USING ERRCODE='22023';
        END IF;
        PERFORM b.slug FROM content.boards b WHERE b.slug IN(OLD.board,NEW.board) ORDER BY b.slug FOR UPDATE NOWAIT;
        IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
        IF EXISTS(SELECT 1 FROM content.threads t WHERE t.board=NEW.board
            AND t.id=NEW.thread_id AND t.content_erased) THEN
            RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='23514';
        END IF;
    END IF;
    IF OLD.content_erased THEN
        IF NOT NEW.content_erased OR NOT NEW.deleted OR NEW.id<>OLD.id
            OR NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id
            OR to_jsonb(NEW) IS DISTINCT FROM to_jsonb(OLD) THEN
            RAISE EXCEPTION 'Erased content cannot be restored.' USING ERRCODE='23514';
        END IF;
    ELSE
        IF current_setting('transaction_isolation')<>'read committed'
            AND ((NOT OLD.deleted AND NEW.deleted) OR NEW.content_erased) THEN
            RAISE EXCEPTION 'Content erasure requires Read Committed.' USING ERRCODE='22023';
        END IF;
        -- A tuple-first operator write must fail rather than wait backwards.
        IF NEW.deleted THEN
            PERFORM b.slug FROM content.boards b WHERE b.slug=OLD.board FOR UPDATE NOWAIT;
            IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
            SELECT t.content_erased INTO parent_erased FROM content.threads t
                WHERE t.board=OLD.board AND t.id=OLD.thread_id;
        END IF;
        IF NOT ((NOT OLD.deleted AND NEW.deleted) OR (NEW.deleted AND coalesce(parent_erased,false))) THEN
            IF NEW.content_erased THEN
                RAISE EXCEPTION 'Erasure requires a whole-deletion transition.' USING ERRCODE='23514';
            END IF;
            RETURN NEW;
        END IF;
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content erasure requires Read Committed.' USING ERRCODE='22023';
        END IF;
        IF NEW.id<>OLD.id OR NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id THEN
            RAISE EXCEPTION 'Deletion cannot reassign content.' USING ERRCODE='23514';
        END IF;
        invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
        IF EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=OLD.board AND b.staff_only)
            AND invoker NOT IN ('board_staff','board_migrator') THEN
            RAISE EXCEPTION 'Content is unavailable.' USING ERRCODE='42501';
        END IF;
        DELETE FROM post_secrets.deletion WHERE post_id=OLD.id;
        DELETE FROM post_secrets.anonymous_posts WHERE post_id=OLD.id;
        DELETE FROM post_secrets.op_replies WHERE post_id=OLD.id;
        DELETE FROM post_secrets.poster_contexts WHERE post_id=OLD.id;
        DELETE FROM post_secrets.posting_history WHERE post_id=OLD.id;
        DELETE FROM staff_identity.discussion_posts WHERE post_id=OLD.id;
        IF OLD.id=OLD.thread_id THEN DELETE FROM post_secrets.op_peers WHERE thread_id=OLD.id; END IF;
    END IF;
    NEW.content_erased:=true;
    NEW.name:=''; NEW.subject:=''; NEW.comment:='';
    NEW.trip:=NULL; NEW.poster_id:=NULL; NEW.json_op_poster_id:=NULL; NEW.capcode:=NULL;
    NEW.country:=NULL; NEW.country_name:=NULL; NEW.board_flag:=NULL; NEW.flag_name:=NULL;
    NEW.wordfilter_payload:=NULL; NEW.wordfilter_search:=NULL;
    NEW.saved_presentation:=NULL; NEW.saved_presentation_search:=NULL;
    NEW.drawing_time_seconds:=NULL; NEW.drawing_source_post_id:=NULL;
    NEW.dice_result:=NULL; NEW.fortune_text:=NULL; NEW.fortune_color:=NULL;
    NEW.file_deleted:=false;
    NEW.comment_format:=0; NEW.staff_authorized_limits:=false; NEW.image_spoiler:=false;
    NEW.board_flag_type:='pol'; NEW.created_at:='epoch'::timestamptz;
    RETURN NEW;
END $$;
GRANT EXECUTE ON FUNCTION content.erasure_visibility_ready() TO board_thread_transfer_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_content_erasure_owner;

-- Keep every ordinary function body unchanged. Only exact, internal xid-bound
-- destination inserts skip request-context stamps/activity. Preserve trigger
-- names, timing, functions and ACLs; refuse unexpected existing WHEN clauses.
DO $$
DECLARE r record; v_definition text; v_had_execute boolean; v_count integer:=0;
BEGIN
    FOR r IN
        SELECT t.oid,t.tgname,t.tgrelid,t.tgfoid,t.tgqual,p.proowner,
            owner.rolname AS owner_name,c.relname
        FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid
        JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_proc p ON p.oid=t.tgfoid
        JOIN pg_roles owner ON owner.oid=p.proowner
        WHERE n.nspname='content' AND NOT t.tgisinternal AND (
            (c.relname='posts' AND t.tgname IN('record_inserted_posting_history','record_ordinary_staff_secrets',
                'record_poster_context','attach_staff_post_receipt','a_apply_staff_capcode','b_stamp_wordfilter_payload',
                'apply_forced_anonymous','apply_post_trip','apply_poster_id','apply_post_flag',
                'apply_posting_randomizer','stamp_comment_format','initial_public_image_spoiler'))
            OR (c.relname='threads' AND t.tgname='stamp_content_autosage'))
        ORDER BY c.relname,t.tgname
    LOOP
        v_count:=v_count+1;
        IF r.tgqual IS NOT NULL THEN RAISE EXCEPTION 'Unexpected transfer trigger shape.'; END IF;
        v_definition:=pg_get_triggerdef(r.oid);
        v_definition:=replace(v_definition,' EXECUTE FUNCTION ',
            CASE WHEN r.relname='threads'
                THEN ' WHEN (NOT content.is_thread_transfer_post(NEW.id,NEW.board,NEW.id)) EXECUTE FUNCTION '
                ELSE ' WHEN (NOT content.is_thread_transfer_post(NEW.id,NEW.board,NEW.thread_id)) EXECUTE FUNCTION ' END);
        v_had_execute:=has_function_privilege('board_migrator',r.tgfoid,'EXECUTE');
        IF NOT v_had_execute THEN
            EXECUTE format('SET LOCAL ROLE %I',r.owner_name);
            EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO board_migrator',r.tgfoid::regprocedure);
            RESET ROLE;
        END IF;
        EXECUTE format('DROP TRIGGER %I ON %s',r.tgname,r.tgrelid::regclass);
        EXECUTE v_definition;
        IF NOT v_had_execute THEN
            EXECUTE format('SET LOCAL ROLE %I',r.owner_name);
            EXECUTE format('REVOKE EXECUTE ON FUNCTION %s FROM board_migrator',r.tgfoid::regprocedure);
            RESET ROLE;
        END IF;
    END LOOP;
    IF v_count<>14 THEN RAISE EXCEPTION 'Incomplete transfer trigger inventory.'; END IF;
END $$;

-- Stable catalog identities survive administrator dump/restore. No persisted
-- OIDs or raw ACL OIDs; exact inventory also rejects added mutating triggers.
SET ROLE board_thread_transfer_owner;
CREATE TABLE post_secrets.thread_transfer_catalog (
    singleton boolean PRIMARY KEY CHECK(singleton),
    definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
REVOKE ALL ON post_secrets.thread_transfer_catalog FROM PUBLIC;

CREATE FUNCTION post_secrets.transfer_acl(p_acl aclitem[]) RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce(jsonb_agg(jsonb_build_array(
    CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END,
    pg_get_userbyid(a.grantor)::text,a.privilege_type,a.is_grantable)
    ORDER BY CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END,
        pg_get_userbyid(a.grantor)::text,a.privilege_type,a.is_grantable),'[]'::jsonb)
 FROM aclexplode(p_acl) a
$$;
CREATE FUNCTION post_secrets.transfer_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
    SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE c.relkind='r' AND (
      (n.nspname='post_secrets' AND (c.relname LIKE 'thread_transfer_%'
        OR c.relname IN('deletion','op_peers','op_replies','posting_history','anonymous_posts','anonymous_sessions','poster_contexts')))
      OR (n.nspname='content' AND c.relname IN('boards','threads','posts','post_media','moderation_audit'))
      OR (n.nspname='staff_identity' AND c.relname IN('accounts','sessions','credentials')))
 ), triggers AS (
    SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
 ), funcs AS (
    SELECT p.* FROM pg_proc p WHERE p.proowner='board_thread_transfer_owner'::regrole
      OR p.oid IN(SELECT t.tgfoid FROM triggers t)
      OR p.oid IN('content.erasure_visibility_ready()'::regprocedure,
        'staff_identity.has_board_access(bigint,text)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
    (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner)::text,
       r.relkind,r.relpersistence,r.relrowsecurity,r.relforcerowsecurity,
       post_secrets.transfer_acl(coalesce(r.relacl,acldefault('r',r.relowner))),
       (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,
          a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid),post_secrets.transfer_acl(a.attacl)) ORDER BY a.attnum)
        FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
        WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
       (SELECT jsonb_agg(jsonb_build_array(k.conname,k.contype,k.convalidated,k.condeferrable,k.condeferred,
           pg_get_constraintdef(k.oid,true)) ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid),
       (SELECT jsonb_agg(jsonb_build_array(p.polname,p.polcmd,p.polpermissive,
          (SELECT jsonb_agg(CASE WHEN x=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x)::text END
             ORDER BY CASE WHEN x=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x)::text END) FROM unnest(p.polroles) x),
          pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)) ORDER BY p.polname)
        FROM pg_policy p WHERE p.polrelid=r.oid)) ORDER BY r.identity) FROM relations r),
    (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
       ORDER BY t.identity,t.tgname) FROM triggers t),
    (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner)::text,
       post_secrets.transfer_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
       ORDER BY p.oid::regprocedure::text) FROM funcs p))::text,'UTF8'))
$$;

CREATE FUNCTION content.text_thread_transfer_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 WITH owner_role AS (
    SELECT oid FROM pg_roles WHERE rolname='board_thread_transfer_owner'
        AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)
 ), required(schema_name,table_name,rls,policy_name) AS (VALUES
    ('content','boards',true,'board_visibility'),('content','posts',true,'post_visibility'),
    ('content','threads',true,'thread_visibility'),('content','post_media',false,''),
    ('post_secrets','deletion',true,'deletion_visibility'),('post_secrets','op_peers',true,'op_peer_visibility'),
    ('post_secrets','op_replies',true,'op_reply_visibility'),('post_secrets','posting_history',false,''),
    ('post_secrets','anonymous_posts',false,''),('post_secrets','anonymous_sessions',false,''),
    ('post_secrets','poster_contexts',false,''),
    ('staff_identity','accounts',false,''),('staff_identity','sessions',false,''),('staff_identity','credentials',false,'')
 ), relations AS (
    SELECT c.*,r.rls,r.policy_name FROM required r JOIN pg_namespace n ON n.nspname=r.schema_name
       JOIN pg_class c ON c.relnamespace=n.oid AND c.relname=r.table_name
 )
 SELECT EXISTS(SELECT 1 FROM owner_role)
    AND content.erasure_visibility_ready()
    AND (SELECT count(*) FROM relations)=14
    AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.relrowsecurity<>r.rls OR r.relforcerowsecurity
        OR r.relkind<>'r' OR r.relpersistence<>'p')
    AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.rls AND NOT EXISTS(
        SELECT 1 FROM pg_policy p WHERE p.polrelid=r.oid AND p.polname=r.policy_name
          AND p.polroles=ARRAY[0]::oid[] AND p.polcmd='*' AND p.polpermissive AND p.polwithcheck IS NULL))
    -- In particular no transfer-owner restrictive policy may hide replies or
    -- private evidence that canonical erasure would subsequently remove.
    AND NOT EXISTS(SELECT 1 FROM relations r JOIN pg_policy p ON p.polrelid=r.oid
        WHERE (p.polname<>r.policy_name OR NOT p.polpermissive) AND EXISTS(
          SELECT 1 FROM unnest(p.polroles) role_id WHERE role_id=0
             OR pg_has_role((SELECT oid FROM owner_role),role_id,'USAGE')))
    AND EXISTS(SELECT 1 FROM pg_policy p WHERE p.polrelid='content.boards'::regclass AND p.polname='board_visibility'
        AND lower(regexp_replace(pg_get_expr(p.polqual,p.polrelid),'[[:space:]()]','','g'))=
            'notstaff_onlyorcurrent_user=anyarray[''board_staff''::name,''board_migrator''::name]')
    AND NOT EXISTS(SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member
        WHERE m.roleid=(SELECT oid FROM owner_role) AND r.rolname<>'board_migrator')
    AND NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.member=(SELECT oid FROM owner_role))
    AND NOT has_schema_privilege('board_thread_transfer_owner','content','CREATE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','post_secrets','CREATE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','staff_identity','CREATE')
    AND (SELECT definition_hash=post_secrets.transfer_catalog_hash()
         FROM post_secrets.thread_transfer_catalog WHERE singleton)
    AND NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
        JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
        CROSS JOIN (VALUES('board_public'),('board_staff'),('board_auth'),('board_media'),
            ('board_media_read'),('board_media_intake'),('board_monitor')) runtime(name)
        WHERE n.nspname='post_secrets' AND c.relname LIKE 'thread_transfer_%'
            AND (has_column_privilege(runtime.name,c.oid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES')
                OR has_table_privilege(runtime.name,c.oid,'DELETE,TRUNCATE,TRIGGER')))
    AND NOT EXISTS(SELECT 1 FROM pg_constraint c JOIN pg_class r ON r.oid=c.conrelid
        JOIN pg_namespace n ON n.oid=r.relnamespace JOIN pg_class target ON target.oid=c.confrelid
        JOIN pg_namespace tn ON tn.oid=target.relnamespace
        WHERE n.nspname='post_secrets' AND r.relname LIKE 'thread_transfer_%' AND c.contype='f'
            AND (tn.nspname<>'post_secrets' OR target.relname NOT IN('thread_transfer_intents','thread_transfer_map')))
$$;
REVOKE ALL ON FUNCTION post_secrets.transfer_acl(aclitem[]),post_secrets.transfer_catalog_hash(),
    content.text_thread_transfer_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.text_thread_transfer_ready() TO board_staff;
CREATE FUNCTION staff_identity.text_thread_transfer_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    SELECT content.text_thread_transfer_ready()
$$;
REVOKE ALL ON FUNCTION staff_identity.text_thread_transfer_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.text_thread_transfer_ready() TO board_auth;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets,staff_identity FROM board_thread_transfer_owner;
SET ROLE board_thread_transfer_owner;
INSERT INTO post_secrets.thread_transfer_catalog VALUES(true,post_secrets.transfer_catalog_hash());
RESET ROLE;
