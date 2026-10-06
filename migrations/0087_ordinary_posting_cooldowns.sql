-- Ordinary timers only (imgboard.php:5887-5900,5933-5960). Duplicate
-- comment/image and pass/client cooldowns are deliberately outside this API.
ALTER TABLE content.boards
    ADD COLUMN posting_reply_seconds integer NOT NULL DEFAULT 60
        CHECK (posting_reply_seconds BETWEEN 0 AND 86400),
    ADD COLUMN posting_image_seconds integer NOT NULL DEFAULT 60
        CHECK (posting_image_seconds BETWEEN 0 AND 86400),
    ADD COLUMN posting_thread_seconds integer NOT NULL DEFAULT 600
        CHECK (posting_thread_seconds BETWEEN 0 AND 86400);
UPDATE content.boards SET posting_image_seconds=30 WHERE NOT worksafe;
UPDATE content.boards SET posting_reply_seconds=30,posting_image_seconds=30,
    posting_thread_seconds=90 WHERE slug IN ('b','pol');
UPDATE content.boards SET posting_reply_seconds=15,posting_image_seconds=15,
    posting_thread_seconds=60 WHERE slug='bant';
UPDATE content.boards SET posting_reply_seconds=90,posting_image_seconds=120 WHERE slug='vg';
UPDATE content.boards SET posting_thread_seconds=3600 WHERE slug IN ('jp','vt');
UPDATE content.boards SET posting_thread_seconds=300 WHERE slug='s4s';
UPDATE content.boards SET posting_thread_seconds=30 WHERE slug='test';

-- Fixed-size synchronization, not one durable lock row per remote identity.
CREATE TABLE post_secrets.posting_actor_gates (
    stripe integer PRIMARY KEY CHECK (stripe BETWEEN 0 AND 4095)
);
INSERT INTO post_secrets.posting_actor_gates SELECT generate_series(0,4095);
CREATE TABLE post_secrets.posting_action_capacity (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton)
);
INSERT INTO post_secrets.posting_action_capacity VALUES(true);
CREATE TABLE post_secrets.posting_history (
    post_id bigint PRIMARY KEY,
    board text NOT NULL REFERENCES content.boards(slug),
    thread_id bigint NOT NULL,
    actor_hash bytea NOT NULL CHECK (octet_length(actor_hash)=32),
    request_at bigint NOT NULL CHECK (request_at BETWEEN 0 AND 9223372036854689407),
    FOREIGN KEY(board,post_id) REFERENCES content.posts(board,id) ON DELETE CASCADE,
    FOREIGN KEY(board,thread_id) REFERENCES content.threads(board,id) ON DELETE CASCADE
);
CREATE INDEX posting_history_board ON post_secrets.posting_history(board);
CREATE INDEX posting_history_reply ON post_secrets.posting_history(board,actor_hash,post_id DESC)
    WHERE post_id<>thread_id;
CREATE INDEX posting_history_op ON post_secrets.posting_history(board,actor_hash,request_at DESC)
    WHERE post_id=thread_id;
CREATE INDEX posting_history_thread ON post_secrets.posting_history(thread_id);
-- No post/thread FK: actions survive content deletion and archive.
-- Removing an entire board also removes its private admission history.
CREATE TABLE post_secrets.posting_thread_actions (
    actor_hash bytea NOT NULL CHECK (octet_length(actor_hash)=32),
    board text NOT NULL REFERENCES content.boards(slug) ON DELETE CASCADE,
    request_at bigint NOT NULL CHECK (request_at BETWEEN 0 AND 9223372036854689407),
    PRIMARY KEY(actor_hash,board)
);
CREATE INDEX posting_thread_actions_expiry ON post_secrets.posting_thread_actions(request_at);
REVOKE ALL ON post_secrets.posting_actor_gates,post_secrets.posting_action_capacity,
    post_secrets.posting_history,post_secrets.posting_thread_actions FROM PUBLIC;
GRANT USAGE ON SCHEMA content,post_secrets TO board_posting_cooldown_owner;
GRANT SELECT,UPDATE(stripe) ON post_secrets.posting_actor_gates TO board_posting_cooldown_owner;
GRANT SELECT,UPDATE(singleton) ON post_secrets.posting_action_capacity TO board_posting_cooldown_owner;
GRANT SELECT,INSERT,DELETE ON post_secrets.posting_history TO board_posting_cooldown_owner;
GRANT SELECT,INSERT,DELETE ON post_secrets.posting_thread_actions TO board_posting_cooldown_owner;
GRANT UPDATE(request_at) ON post_secrets.posting_thread_actions TO board_posting_cooldown_owner;
GRANT SELECT(slug,staff_only,posting_reply_seconds,posting_image_seconds,posting_thread_seconds),UPDATE(slug)
    ON content.boards TO board_posting_cooldown_owner;
GRANT SELECT(id,board,deleted,archived_at) ON content.threads TO board_posting_cooldown_owner;
-- RLS stays enabled and the owner remains NOBYPASSRLS. The existing post and
-- thread visibility policies follow this owner's board visibility. Functions
-- explicitly preserve the invoker's private-board boundary below.
CREATE POLICY posting_cooldown_board_read ON content.boards
    FOR SELECT TO board_posting_cooldown_owner USING(true);
CREATE POLICY posting_cooldown_board_lock ON content.boards
    FOR UPDATE TO board_posting_cooldown_owner USING(true) WITH CHECK(true);


CREATE FUNCTION content.lock_posting_actor(p_actor bytea,p_new_thread boolean)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_new_thread IS NULL THEN
        RAISE EXCEPTION 'Invalid posting actor.' USING ERRCODE='23514';
    END IF;
    -- All posting callers: actor stripe, OP capacity (if needed), board, thread.
    -- Full actor digests are compared in history; stripe collisions only wait.
    PERFORM g.stripe FROM post_secrets.posting_actor_gates g
        WHERE g.stripe=(get_byte(p_actor,0)*256+get_byte(p_actor,1))%4096 FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Posting capacity is unavailable.' USING ERRCODE='P0087';
    END IF;
    IF p_new_thread THEN
        PERFORM c.singleton FROM post_secrets.posting_action_capacity c WHERE c.singleton FOR UPDATE;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'Posting capacity is unavailable.' USING ERRCODE='P0087';
        END IF;
    END IF;
END $$;

CREATE FUNCTION content.check_posting_cooldown(
    p_actor bytea,p_board text,p_thread bigint,p_has_attachment boolean,p_request_at bigint
) RETURNS TABLE(kind text,remaining_seconds bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_reply integer; v_image integer; v_thread integer;
    v_delay integer; v_last bigint; v_now bigint; v_staff_only boolean;
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_thread IS NULL OR p_thread<0
        OR p_has_attachment IS NULL OR p_request_at IS NULL
        OR p_request_at NOT BETWEEN 0 AND 9223372036854689407 THEN
        RAISE EXCEPTION 'Invalid posting context.' USING ERRCODE='23514';
    END IF;
    -- The caller already holds its actor/action gates. Reuse the authoritative
    -- board lock so operator changes and concurrent board mutations serialize.
    SELECT b.posting_reply_seconds,b.posting_image_seconds,b.posting_thread_seconds,b.staff_only
        INTO v_reply,v_image,v_thread,v_staff_only FROM content.boards b WHERE b.slug=p_board FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002'; END IF;
    IF v_staff_only AND (CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END) NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002';
    END IF;
    IF p_thread>0 THEN
        SELECT h.request_at INTO v_last FROM post_secrets.posting_history h
            WHERE h.actor_hash=p_actor AND h.board=p_board AND h.post_id<>h.thread_id
            ORDER BY h.post_id DESC LIMIT 1;
        v_delay:=CASE WHEN p_has_attachment THEN v_image ELSE v_reply END;
        IF v_last>p_request_at-v_delay THEN
            kind:=CASE WHEN p_has_attachment THEN 'image' ELSE 'reply' END;
            remaining_seconds:=v_last+v_delay-p_request_at;
            RETURN NEXT;
        END IF;
        RETURN;
    END IF;
    -- Sticky status does not discard the surviving OP's posting identity.
    SELECT max(h.request_at) INTO v_last FROM post_secrets.posting_history h
        WHERE h.actor_hash=p_actor AND h.board=p_board AND h.post_id=h.thread_id;
    IF v_last>p_request_at-v_thread THEN
        kind:='thread'; remaining_seconds:=v_last+v_thread-p_request_at;
        RETURN NEXT; RETURN;
    END IF;
    -- Cross-board uses the DB clock after contended locks, not request time.
    -- Its lower edge is inclusive, unlike the same-board strict comparison.
    v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
    SELECT max(a.request_at) INTO v_last FROM post_secrets.posting_thread_actions a
        WHERE a.actor_hash=p_actor AND a.board<>p_board AND a.request_at>=v_now-300;
    IF v_last IS NOT NULL THEN
        kind:='cross_board_thread'; remaining_seconds:=v_last+301-v_now;
        RETURN NEXT;
    END IF;
END $$;

-- Only a newly inserted post can acquire history. There is no runtime function
-- that can attach an actor to existing, restored or historical content. The
-- actor context remains application-supplied, just like the old SQL argument;
-- it is not a cryptographic proof against a compromised application role.
CREATE FUNCTION content.record_inserted_posting_history()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_actor bytea; v_encoded text; v_invoker text;
    v_request bigint; v_now bigint; v_staff_only boolean;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    v_encoded:=current_setting('board.posting_actor',true);
    -- Historical imports have no recoverable identity. An operator may supply
    -- an explicit actor, using exactly the same validation and capacity path.
    -- The attachment definer retains its original board_public invoker here.
    IF v_invoker='board_migrator' AND coalesce(v_encoded,'')='' THEN RETURN NEW; END IF;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Posting history registration is unavailable.' USING ERRCODE='23514';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Posting history requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF v_encoded IS NULL OR octet_length(v_encoded)<>64 OR v_encoded !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Invalid posting actor.' USING ERRCODE='23514';
    END IF;
    v_actor:=decode(v_encoded,'hex');
    -- The normal writer already owns these gates before any board/thread,
    -- admission, session or media lock. Late reentry never waits: a direct SQL
    -- writer that skipped that order fails closed instead of creating a cycle.
    PERFORM g.stripe FROM post_secrets.posting_actor_gates g
        WHERE g.stripe=(get_byte(v_actor,0)*256+get_byte(v_actor,1))%4096 FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Posting capacity is unavailable.' USING ERRCODE='P0087';
    END IF;
    IF NEW.id=NEW.thread_id THEN
        PERFORM c.singleton FROM post_secrets.posting_action_capacity c
            WHERE c.singleton FOR UPDATE NOWAIT;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'Posting capacity is unavailable.' USING ERRCODE='P0087';
        END IF;
    END IF;
    SELECT b.staff_only INTO v_staff_only FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE;
    IF NOT FOUND OR (v_staff_only AND v_invoker NOT IN ('board_staff','board_migrator')) THEN
        RAISE EXCEPTION 'Posting history registration is unavailable.' USING ERRCODE='23514';
    END IF;
    IF NEW.deleted OR NOT EXISTS(SELECT 1 FROM content.threads t
        WHERE t.id=NEW.thread_id AND t.board=NEW.board AND NOT t.deleted AND t.archived_at IS NULL) THEN
        RAISE EXCEPTION 'Posting history registration is unavailable.' USING ERRCODE='23514';
    END IF;
    v_request:=floor(extract(epoch FROM NEW.created_at))::bigint;
    -- A fixed 100,000-row per-board budget fails closed, never evicts live
    -- identity. There is intentionally no TTL: a later policy increase or a
    -- reversed request-time ordering must still see surviving source history.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.posting_history h
        WHERE h.board=NEW.board LIMIT 100000) bounded)>=100000 THEN
        RAISE EXCEPTION 'Posting history capacity is exhausted.' USING ERRCODE='P0087';
    END IF;
    -- The only runtime writer derives all three keys from NEW; the
    -- composite foreign keys additionally enforce both board associations.
    INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at)
        VALUES(NEW.id,NEW.board,NEW.thread_id,v_actor,v_request);
    IF NEW.id=NEW.thread_id THEN
        IF NOT EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions a
            WHERE a.actor_hash=v_actor AND a.board=NEW.board) THEN
            v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
            DELETE FROM post_secrets.posting_thread_actions a WHERE (a.actor_hash,a.board) IN (
                SELECT expired.actor_hash,expired.board FROM post_secrets.posting_thread_actions expired
                WHERE expired.request_at<v_now-300 ORDER BY expired.request_at
                LIMIT 64 FOR UPDATE SKIP LOCKED
            );
            IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.posting_thread_actions
                LIMIT 100000) bounded)>=100000 THEN
                RAISE EXCEPTION 'Posting action capacity is exhausted.' USING ERRCODE='P0087';
            END IF;
        END IF;
        INSERT INTO post_secrets.posting_thread_actions AS a(actor_hash,board,request_at)
            VALUES(v_actor,NEW.board,v_request)
            ON CONFLICT(actor_hash,board) DO UPDATE SET request_at=greatest(a.request_at,EXCLUDED.request_at);
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER record_inserted_posting_history AFTER INSERT ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.record_inserted_posting_history();

CREATE FUNCTION content.remove_posting_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF NEW.deleted THEN DELETE FROM post_secrets.posting_history WHERE post_id=NEW.id; END IF;
    RETURN NEW;
END $$;
CREATE FUNCTION content.remove_thread_posting_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF NEW.deleted OR NEW.archived_at IS NOT NULL THEN
        DELETE FROM post_secrets.posting_history WHERE thread_id=NEW.id;
    END IF;
    RETURN NEW;
END $$;
-- File-only deletion touches no history. Undelete never reconstructs identity.
-- Hard deletes cascade; successful action rows have no post/thread foreign key.
CREATE TRIGGER remove_posting_history AFTER UPDATE OF deleted ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.remove_posting_history();
CREATE TRIGGER remove_thread_posting_history AFTER UPDATE OF deleted,archived_at ON content.threads
    FOR EACH ROW EXECUTE FUNCTION content.remove_thread_posting_history();
REVOKE ALL ON FUNCTION content.lock_posting_actor(bytea,boolean),
    content.check_posting_cooldown(bytea,text,bigint,boolean,bigint),
    content.record_inserted_posting_history(),content.remove_posting_history(),
    content.remove_thread_posting_history() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.lock_posting_actor(bytea,boolean),
    content.check_posting_cooldown(bytea,text,bigint,boolean,bigint) TO board_public,board_staff;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
ALTER FUNCTION content.lock_posting_actor(bytea,boolean) OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.check_posting_cooldown(bytea,text,bigint,boolean,bigint) OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.record_inserted_posting_history() OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.remove_posting_history() OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.remove_thread_posting_history() OWNER TO board_posting_cooldown_owner;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;
-- No backfill: existing public content contains no recoverable actor identity.
