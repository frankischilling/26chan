-- Static source staff IDs derive only from the consumed public badge.
-- Keep all historical eight-character IDs and null fields unchanged.
ALTER TABLE content.posts DROP CONSTRAINT posts_poster_id_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_poster_id_check CHECK (
    poster_id IS NULL OR poster_id ~ '^[+/0-9A-Za-z]{8}$'
    OR (capcode IS NOT NULL AND poster_id=CASE capcode
        WHEN 'mod' THEN 'Mod'
        WHEN 'admin' THEN 'Admin'
        WHEN 'admin_highlight' THEN 'Admin'
        WHEN 'manager' THEN 'Manager'
        WHEN 'developer' THEN 'Developer'
        WHEN 'founder' THEN 'Founder'
        ELSE '' END));

CREATE OR REPLACE FUNCTION content.apply_poster_id() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_enabled boolean;
BEGIN
    SELECT user_ids INTO v_enabled FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    IF NEW.capcode IS NOT NULL THEN
        NEW.poster_id:=CASE WHEN v_enabled THEN CASE NEW.capcode
            WHEN 'mod' THEN 'Mod'
            WHEN 'admin' THEN 'Admin'
            WHEN 'admin_highlight' THEN 'Admin'
            WHEN 'manager' THEN 'Manager'
            WHEN 'developer' THEN 'Developer'
            WHEN 'founder' THEN 'Founder'
            ELSE NULL END ELSE NULL END;
        RETURN NEW;
    END IF;
    NEW.poster_id:=CASE WHEN v_enabled THEN nullif(current_setting('board.poster_id',true),'') ELSE NULL END;
    IF v_enabled AND NEW.poster_id IS NULL THEN
        RAISE EXCEPTION 'Poster identity is unavailable.' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_poster_id() FROM PUBLIC;
