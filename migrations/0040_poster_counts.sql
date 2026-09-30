CREATE TABLE post_secrets.poster_contexts (
    post_id bigint PRIMARY KEY REFERENCES content.posts(id) ON DELETE CASCADE,
    thread_id bigint NOT NULL REFERENCES content.threads(id) ON DELETE CASCADE,
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint)=32),
    epoch bytea NOT NULL CHECK (octet_length(epoch)=32)
);
CREATE INDEX poster_contexts_thread ON post_secrets.poster_contexts(thread_id);
REVOKE ALL ON post_secrets.poster_contexts FROM PUBLIC;
GRANT USAGE ON SCHEMA content,post_secrets TO board_poster_count_owner;
GRANT SELECT(id,board,thread_id,deleted) ON content.posts TO board_poster_count_owner;
GRANT SELECT(id,board,deleted,archived_at) ON content.threads TO board_poster_count_owner;
GRANT SELECT,INSERT,DELETE ON post_secrets.poster_contexts TO board_poster_count_owner;

CREATE FUNCTION content.record_poster_context() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_fingerprint text; v_epoch text;
BEGIN
    v_fingerprint := nullif(current_setting('board.poster_fingerprint',true),'');
    v_epoch := nullif(current_setting('board.poster_epoch',true),'');
    IF v_fingerprint IS NULL AND v_epoch IS NULL THEN RETURN NEW; END IF;
    IF v_fingerprint IS NULL OR v_epoch IS NULL
       OR v_fingerprint !~ '^[0-9a-f]{64}$' OR v_epoch !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Poster count context is invalid.' USING ERRCODE='23514';
    END IF;
    IF NOT NEW.deleted AND EXISTS (SELECT 1 FROM content.threads
        WHERE id=NEW.thread_id AND board=NEW.board AND NOT deleted AND archived_at IS NULL) THEN
        INSERT INTO post_secrets.poster_contexts(post_id,thread_id,fingerprint,epoch)
            VALUES(NEW.id,NEW.thread_id,decode(v_fingerprint,'hex'),decode(v_epoch,'hex'));
    END IF;
    RETURN NEW;
END $$;
CREATE FUNCTION content.remove_post_context() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF NEW.deleted THEN DELETE FROM post_secrets.poster_contexts WHERE post_id=NEW.id; END IF;
    RETURN NEW;
END $$;
CREATE FUNCTION content.remove_thread_contexts() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF NEW.deleted OR NEW.archived_at IS NOT NULL THEN
        DELETE FROM post_secrets.poster_contexts WHERE thread_id=NEW.id;
    END IF;
    RETURN NEW;
END $$;
CREATE FUNCTION content.unique_posters(v_board text,v_thread bigint) RETURNS integer
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    SELECT CASE WHEN count(p.id) BETWEEN 1 AND 1001 AND count(c.post_id)=count(p.id)
        AND count(DISTINCT c.epoch)=1 THEN count(DISTINCT c.fingerprint)::integer ELSE NULL END
    FROM content.threads t JOIN content.posts p ON p.thread_id=t.id AND p.board=t.board AND NOT p.deleted
    LEFT JOIN post_secrets.poster_contexts c ON c.post_id=p.id AND c.thread_id=t.id
    WHERE t.board=v_board AND t.id=v_thread AND NOT t.deleted AND t.archived_at IS NULL
    GROUP BY t.id
$$;
CREATE TRIGGER record_poster_context AFTER INSERT ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.record_poster_context();
CREATE TRIGGER remove_post_context AFTER UPDATE OF deleted ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.remove_post_context();
CREATE TRIGGER remove_thread_contexts AFTER UPDATE OF deleted,archived_at ON content.threads
    FOR EACH ROW EXECUTE FUNCTION content.remove_thread_contexts();
REVOKE ALL ON FUNCTION content.record_poster_context(),content.remove_post_context(),
    content.remove_thread_contexts(),content.unique_posters(text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.unique_posters(text,bigint) TO board_public;
GRANT CREATE ON SCHEMA content TO board_poster_count_owner;
ALTER FUNCTION content.record_poster_context() OWNER TO board_poster_count_owner;
ALTER FUNCTION content.remove_post_context() OWNER TO board_poster_count_owner;
ALTER FUNCTION content.remove_thread_contexts() OWNER TO board_poster_count_owner;
ALTER FUNCTION content.unique_posters(text,bigint) OWNER TO board_poster_count_owner;
REVOKE CREATE ON SCHEMA content FROM board_poster_count_owner;
