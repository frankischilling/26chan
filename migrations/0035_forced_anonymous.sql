-- FORCED_ANON defaults off globally and in every active source board config.
ALTER TABLE content.boards ADD COLUMN forced_anon boolean NOT NULL DEFAULT false;
GRANT SELECT(forced_anon) ON content.boards TO board_attachment_owner;

-- Apply the ordinary public posting policy to both direct and scoped inserts.
-- Updates and historical rows retain the identity that was saved at posting.
CREATE FUNCTION content.apply_forced_anonymous() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_forced boolean;
BEGIN
    SELECT forced_anon INTO v_forced FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    IF v_forced THEN
        NEW.name := 'Anonymous';
        NEW.subject := '';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_forced_anonymous() FROM PUBLIC;
CREATE TRIGGER apply_forced_anonymous BEFORE INSERT ON content.posts
FOR EACH ROW EXECUTE FUNCTION content.apply_forced_anonymous();
