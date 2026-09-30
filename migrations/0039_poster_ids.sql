ALTER TABLE content.boards ADD COLUMN user_ids boolean NOT NULL DEFAULT false;
ALTER TABLE content.posts ADD COLUMN poster_id text
CHECK (poster_id IS NULL OR poster_id ~ '^[+/0-9A-Za-z]{8}$');

CREATE FUNCTION content.apply_poster_id() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE v_enabled boolean;
BEGIN
    SELECT user_ids INTO v_enabled FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    NEW.poster_id := CASE WHEN v_enabled THEN nullif(current_setting('board.poster_id', true), '') ELSE NULL END;
    IF v_enabled AND NEW.poster_id IS NULL THEN
        RAISE EXCEPTION 'Poster identity is unavailable.' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_poster_id() FROM PUBLIC;
CREATE TRIGGER apply_poster_id BEFORE INSERT ON content.posts
FOR EACH ROW EXECUTE FUNCTION content.apply_poster_id();

-- The attachment trigger reads only this additional public board setting.
GRANT SELECT(user_ids) ON content.boards TO board_attachment_owner;
