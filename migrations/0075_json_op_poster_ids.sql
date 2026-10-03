-- JSON recomputes the ordinary OP's network label even when HTML uses Heaven.
-- Retain only the already-derived public label; do not backfill saved history.
ALTER TABLE content.posts ADD COLUMN json_op_poster_id text;
ALTER TABLE content.posts ADD CONSTRAINT posts_json_op_poster_id_check CHECK (
    json_op_poster_id IS NULL OR (
        json_op_poster_id ~ '^[+/0-9A-Za-z]{8}$' AND id=thread_id
        AND capcode IS NULL AND poster_id IS NOT NULL));

CREATE OR REPLACE FUNCTION content.apply_poster_id() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_enabled boolean; v_meta boolean; v_no_heaven boolean; v_network text;
BEGIN
    NEW.json_op_poster_id:=NULL;
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
    IF NEW.id=NEW.thread_id THEN NEW.json_op_poster_id:=v_network; END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_poster_id() FROM PUBLIC;
