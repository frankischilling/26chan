ALTER TABLE content.boards
    ADD COLUMN op_bump_limit boolean NOT NULL DEFAULT true,
    ADD COLUMN op_bump_initial_seconds integer NOT NULL DEFAULT 900 CHECK (op_bump_initial_seconds >= 0),
    ADD COLUMN op_bump_repeat_seconds integer NOT NULL DEFAULT 300 CHECK (op_bump_repeat_seconds >= 0);

-- Sensitive posting data, never part of the public content projection.
-- Record one transport address per active OP, not the addresses of every reply.
CREATE TABLE post_secrets.op_peers (
    thread_id bigint PRIMARY KEY REFERENCES content.threads(id) ON DELETE CASCADE,
    peer inet NOT NULL CHECK (masklen(peer) = CASE family(peer) WHEN 4 THEN 32 ELSE 128 END)
);
CREATE TABLE post_secrets.op_replies (
    post_id bigint PRIMARY KEY REFERENCES content.posts(id) ON DELETE CASCADE,
    thread_id bigint NOT NULL REFERENCES post_secrets.op_peers(thread_id) ON DELETE CASCADE
);
CREATE INDEX op_replies_latest ON post_secrets.op_replies(thread_id,post_id DESC);
REVOKE ALL ON post_secrets.op_peers,post_secrets.op_replies FROM PUBLIC;
GRANT SELECT,INSERT ON post_secrets.op_peers,post_secrets.op_replies TO board_public;

-- Cleanup is driven by actual content transitions, including staff deletion
-- and archive eviction. Runtimes do not gain a general private-data DELETE grant.
CREATE FUNCTION post_secrets.purge_op_thread() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF NEW.deleted OR NEW.archived_at IS NOT NULL THEN
        DELETE FROM post_secrets.op_peers WHERE thread_id=NEW.id;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION post_secrets.purge_op_thread() FROM PUBLIC;
CREATE TRIGGER purge_op_thread AFTER UPDATE OF deleted,archived_at ON content.threads
FOR EACH ROW EXECUTE FUNCTION post_secrets.purge_op_thread();

CREATE FUNCTION post_secrets.purge_op_post() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF NEW.deleted THEN
        DELETE FROM post_secrets.op_replies WHERE post_id=NEW.id;
        IF NEW.id=NEW.thread_id THEN
            DELETE FROM post_secrets.op_peers WHERE thread_id=NEW.id;
        END IF;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION post_secrets.purge_op_post() FROM PUBLIC;
CREATE TRIGGER purge_op_post AFTER UPDATE OF deleted ON content.posts
FOR EACH ROW EXECUTE FUNCTION post_secrets.purge_op_post();
