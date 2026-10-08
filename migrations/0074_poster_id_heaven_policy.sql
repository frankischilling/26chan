-- Source sage display policy; preserve all saved post IDs and staff authority.
ALTER TABLE content.boards ADD COLUMN poster_id_no_heaven boolean NOT NULL DEFAULT false;
UPDATE content.boards SET poster_id_no_heaven=true WHERE slug IN ('bant','biz','pol','qst','soc');

ALTER TABLE content.posts DROP CONSTRAINT posts_poster_id_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_poster_id_check CHECK (
    poster_id IS NULL OR poster_id ~ '^[+/0-9A-Za-z]{8}$'
    OR (poster_id='Heaven' AND capcode IS NULL)
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
DECLARE v_enabled boolean; v_meta boolean; v_no_heaven boolean; v_network text;
BEGIN
    SELECT user_ids,meta_board,poster_id_no_heaven INTO v_enabled,v_meta,v_no_heaven
        FROM content.boards WHERE slug=NEW.board FOR SHARE;
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
    IF NOT v_enabled THEN NEW.poster_id:=NULL; RETURN NEW; END IF;
    v_network:=nullif(current_setting('board.poster_id',true),'');
    IF v_network IS NULL OR v_network !~ '^[+/0-9A-Za-z]{8}$' THEN
        RAISE EXCEPTION 'Poster identity is unavailable.' USING ERRCODE='23514';
    END IF;
    NEW.poster_id:=CASE WHEN current_setting('board.post_sage',true)='true'
        AND NOT v_meta AND NOT v_no_heaven THEN 'Heaven' ELSE v_network END;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_poster_id() FROM PUBLIC;
