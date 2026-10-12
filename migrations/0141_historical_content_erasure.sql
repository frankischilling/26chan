-- Explicit operator cleanup of already-deleted payload. Installing this
-- migration does not erase historical rows, reports, audit data or media.
-- Public/staff permissions are unchanged; no trigger is disabled.
DO $historical_erasure_preflight$
BEGIN
    SET LOCAL ROLE board_thread_transfer_owner;
    PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
    IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Text transfer catalog is not ready before historical erasure.';
    END IF;
    RESET ROLE;
END $historical_erasure_preflight$;

GRANT CREATE ON SCHEMA post_secrets TO board_content_erasure_owner;
SET LOCAL ROLE board_content_erasure_owner;
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
        invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
        -- Only the existing migration operator may explicitly erase an old
        -- tombstone. Ordinary deletion authority and live transitions do not change.
        IF NOT ((NOT OLD.deleted AND NEW.deleted) OR (NEW.deleted AND coalesce(parent_erased,false))
            OR (OLD.deleted AND NEW.deleted AND NEW.content_erased AND invoker='board_migrator')) THEN
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

CREATE OR REPLACE FUNCTION post_secrets.guard_thread_erasure() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE historical boolean;
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.content_erased OR NEW.deleted THEN
            RAISE EXCEPTION 'Cannot insert deleted content.' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    -- No application operation moves a thread identity. Operators must not
    -- move it either: author-link writers hold the captured board lock, and
    -- changing that mapping would invalidate their post-lock checks.
    IF NEW.id<>OLD.id OR NEW.board<>OLD.board THEN
        RAISE EXCEPTION 'Thread identity cannot be reassigned.' USING ERRCODE='23514';
    END IF;
    IF OLD.content_erased THEN
        IF NOT NEW.content_erased OR NOT NEW.deleted OR NEW.id<>OLD.id OR NEW.board<>OLD.board
            OR (to_jsonb(NEW)-'http_modified_at') IS DISTINCT FROM (to_jsonb(OLD)-'http_modified_at') THEN
            RAISE EXCEPTION 'Erased content cannot be restored.' USING ERRCODE='23514';
        END IF;
    ELSIF NEW.deleted AND (NOT OLD.deleted OR (NEW.content_erased AND
        CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END='board_migrator')) THEN
        historical:=OLD.deleted;
        -- Historical children are processed in bounded post batches first.
        -- Never turn one operator thread update into an unbounded legacy sweep.
        IF historical AND EXISTS(SELECT 1 FROM content.posts p
            WHERE p.board=OLD.board AND p.thread_id=OLD.id AND NOT p.content_erased) THEN
            RAISE EXCEPTION 'Erase historical descendants before their thread.' USING ERRCODE='23514';
        END IF;
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content erasure requires Read Committed.' USING ERRCODE='22023';
        END IF;
        PERFORM b.slug FROM content.boards b WHERE b.slug=OLD.board FOR UPDATE NOWAIT;
        IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
        IF NEW.id<>OLD.id OR NEW.board<>OLD.board THEN
            RAISE EXCEPTION 'Deletion cannot reassign content.' USING ERRCODE='23514';
        END IF;
    ELSE
        IF NEW.content_erased THEN
            RAISE EXCEPTION 'Erasure requires a whole-deletion transition.' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    NEW.content_erased:=true;
    NEW.created_at:='epoch'::timestamptz; NEW.bumped_at:='epoch'::timestamptz;
    NEW.modified_at:='epoch'::timestamptz; NEW.http_modified_at:='epoch'::timestamptz;
    NEW.reply_count:=0; NEW.sticky:=false; NEW.closed:=false; NEW.permasage:=false;
    NEW.permaage:=false; NEW.undead:=false; NEW.sticky_rank:=0;
    NEW.archived_at:=NULL; NEW.archive_expires_at:=NULL;
    RETURN NEW;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_content_erasure_owner;

SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash()
    WHERE singleton;
DO $$ BEGIN
    IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Text transfer catalog is not ready after historical erasure.';
    END IF;
END $$;
RESET ROLE;
