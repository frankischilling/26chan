-- Source imgboard.php only appends Oekaki metadata when an image was accepted,
-- the painter and replay board switches are enabled, and oe_time was supplied.
-- An imported edit deliberately has no replay. Preserve its optional time and
-- source as typed post data, separate from the user's comment and media bytes.
ALTER TABLE content.posts
    ADD COLUMN drawing_time_seconds integer
        CHECK (drawing_time_seconds BETWEEN 1 AND 5184000),
    ADD COLUMN drawing_source_post_id bigint
        CHECK (drawing_source_post_id > 0),
    ADD CONSTRAINT drawing_source_requires_time CHECK (
        drawing_source_post_id IS NULL OR drawing_time_seconds IS NOT NULL
    );

-- Only the already trusted attachment inserter may stamp these two columns.
-- Public posting has no direct INSERT/UPDATE permission on either column.
GRANT SELECT(oekaki, oekaki_replays) ON content.boards TO board_attachment_owner;
GRANT UPDATE(drawing_time_seconds, drawing_source_post_id)
    ON content.posts TO board_attachment_owner;

-- The attachment owner sees the original post/media rows, including a deleted
-- image tombstone. This is the source's tim != 0 test, not media fetch authority.
-- Transaction-local settings carry untrusted annotation values only; they do
-- not authorize an upload or writes to an arbitrary existing post.
CREATE FUNCTION content.stamp_drawing_annotation() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_time text := current_setting('board.drawing_time_seconds', true);
    v_source text := current_setting('board.drawing_source_post_id', true);
    v_seconds integer;
    v_board text;
    v_thread bigint;
    v_source_id bigint;
BEGIN
    IF v_time IS NULL OR octet_length(v_time)>7 OR v_time !~ '^[1-9][0-9]{0,6}$' THEN
        RETURN NEW;
    END IF;
    v_seconds := v_time::integer;
    IF v_seconds>5184000 THEN
        RETURN NEW;
    END IF;
    -- A real post-media INSERT has already happened; the enclosing authorized
    -- inserter checked its one-use upload, published asset and image limit.
    SELECT p.board,p.thread_id INTO v_board,v_thread FROM content.posts p
        JOIN content.boards b ON b.slug=p.board
        WHERE p.id=NEW.post_id AND NOT p.deleted
            AND b.oekaki AND b.oekaki_replays;
    IF NOT FOUND THEN
        RETURN NEW;
    END IF;

    IF NEW.post_id<>v_thread AND v_source IS NOT NULL AND octet_length(v_source) BETWEEN 1 AND 19
        AND v_source ~ '^[1-9][0-9]{0,18}$' THEN
        IF v_source::numeric<=9223372036854775807 THEN
            v_source_id := v_source::bigint;
            -- Source accepts any same-board OP, or a reply to the target
            -- thread. File-deleted sources still have nonzero tim; deleted
            -- posts and this newly inserted post cannot be source records.
            PERFORM 1 FROM content.posts p
                JOIN content.post_media m ON m.post_id=p.id
                WHERE p.board=v_board AND p.id=v_source_id AND NOT p.deleted
                    AND p.id<>NEW.post_id AND m.tim<>0
                    AND (p.id=p.thread_id OR p.thread_id=v_thread);
            IF NOT FOUND THEN
                v_source_id := NULL;
            END IF;
        END IF;
    END IF;
    UPDATE content.posts p SET drawing_time_seconds=v_seconds,
        drawing_source_post_id=v_source_id
        WHERE p.id=NEW.post_id AND p.board=v_board AND p.thread_id=v_thread
            AND NOT p.deleted;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Drawing annotation target is unavailable.' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.stamp_drawing_annotation()
    FROM PUBLIC,board_public,board_staff,board_auth;
CREATE TRIGGER stamp_drawing_annotation AFTER INSERT ON content.post_media
    FOR EACH ROW EXECUTE FUNCTION content.stamp_drawing_annotation();
-- Final owner is an existing NOLOGIN role with attachment INSERT authority.
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
ALTER FUNCTION content.stamp_drawing_annotation() OWNER TO board_attachment_owner;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

-- Public search indexes the same visible annotation words as the post renderer,
-- without adding generated text to the saved comment or wordfilter cache.
-- This projection depends only on its numeric arguments. Invalid or absent
-- times have no display/search text; positive source IDs add the plain link text.
CREATE FUNCTION content.drawing_search_text(p_seconds integer, p_source bigint)
RETURNS text LANGUAGE sql IMMUTABLE PARALLEL SAFE SECURITY INVOKER
SET search_path=pg_catalog AS $drawing_search$
    SELECT CASE WHEN p_seconds BETWEEN 1 AND 5184000 THEN
        'Oekaki Post (Time: ' ||
        CASE
            WHEN p_seconds < 60 THEN p_seconds::text || 's'
            WHEN p_seconds < 3600 THEN ((p_seconds + 30) / 60)::text || 'm'
            ELSE (p_seconds / 3600)::text || 'h '
                || (((p_seconds % 3600) + 30) / 60)::text || 'm'
        END ||
        CASE WHEN p_source > 0 THEN ', Source: >>' || p_source::text ELSE '' END ||
        ')'
    END
$drawing_search$;
REVOKE ALL ON FUNCTION content.drawing_search_text(integer,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth;
GRANT EXECUTE ON FUNCTION content.drawing_search_text(integer,bigint) TO board_public;
