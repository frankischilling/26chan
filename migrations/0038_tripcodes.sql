ALTER TABLE content.posts ADD COLUMN trip text
CHECK (trip IS NULL OR trip ~ '^(![./0-9A-Za-z]{10}|!![+/0-9A-Za-z]{11})$');

-- Both ordinary inserts and the existing scoped attachment function pass
-- through this trigger. The session value lives only within the posting tx.
CREATE FUNCTION content.apply_post_trip() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE v_forced boolean;
BEGIN
    SELECT forced_anon INTO v_forced FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    NEW.trip := CASE WHEN v_forced THEN NULL
        ELSE nullif(current_setting('board.post_trip', true), '') END;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_post_trip() FROM PUBLIC;
CREATE TRIGGER apply_post_trip BEFORE INSERT ON content.posts
FOR EACH ROW EXECUTE FUNCTION content.apply_post_trip();
