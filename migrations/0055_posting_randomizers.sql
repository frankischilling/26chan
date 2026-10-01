-- Source posting options are operator-owned per-board policy. Generated values
-- are retained as post metadata so every later projection reuses one result.
ALTER TABLE content.boards
    ADD COLUMN dice_roll boolean NOT NULL DEFAULT false,
    ADD COLUMN fortune_trip boolean NOT NULL DEFAULT false;

ALTER TABLE content.posts
    ADD COLUMN dice_result text CHECK (
        dice_result IS NULL OR
        (octet_length(dice_result) BETWEEN 1 AND 1024 AND dice_result !~ '[[:cntrl:]]')
    ),
    ADD COLUMN fortune_text text CHECK (
        fortune_text IS NULL OR
        (octet_length(fortune_text) BETWEEN 1 AND 256 AND fortune_text !~ '[[:cntrl:]]')
    ),
    ADD COLUMN fortune_color text CHECK (
        fortune_color IS NULL OR fortune_color ~ '^#[0-9a-f]{6}$'
    ),
    ADD CONSTRAINT paired_posting_randomizers CHECK (
        (fortune_text IS NULL) = (fortune_color IS NULL)
        AND NOT (dice_result IS NOT NULL AND fortune_text IS NOT NULL)
    );

UPDATE content.boards SET
    dice_roll = slug = ANY(ARRAY['b','mlp','qst','tg']::text[]),
    fortune_trip = slug = ANY(ARRAY['b','s4s']::text[]);

CREATE FUNCTION content.apply_posting_randomizer() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_dice_enabled boolean;
    v_fortune_enabled boolean;
    v_dice text;
    v_fortune text;
    v_color text;
BEGIN
    SELECT dice_roll,fortune_trip INTO v_dice_enabled,v_fortune_enabled
      FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    v_dice := nullif(current_setting('board.dice_result',true),'');
    v_fortune := nullif(current_setting('board.fortune_text',true),'');
    v_color := nullif(current_setting('board.fortune_color',true),'');
    IF v_dice IS NOT NULL AND NOT v_dice_enabled THEN
        RAISE EXCEPTION 'Dice rolls are disabled on this board.' USING ERRCODE='23514';
    END IF;
    IF (v_fortune IS NOT NULL OR v_color IS NOT NULL) AND NOT v_fortune_enabled THEN
        RAISE EXCEPTION 'Fortunes are disabled on this board.' USING ERRCODE='23514';
    END IF;
    IF (v_fortune IS NULL) <> (v_color IS NULL)
       OR (v_dice IS NOT NULL AND v_fortune IS NOT NULL) THEN
        RAISE EXCEPTION 'Invalid posting randomizer metadata.' USING ERRCODE='23514';
    END IF;
    NEW.dice_result := v_dice;
    NEW.fortune_text := v_fortune;
    NEW.fortune_color := v_color;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_posting_randomizer() FROM PUBLIC;
CREATE TRIGGER apply_posting_randomizer BEFORE INSERT ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.apply_posting_randomizer();

-- Attachment insertion runs under this constrained owner and its trigger must
-- be able to re-check the board feature bits.
GRANT SELECT(dice_roll,fortune_trip) ON content.boards TO board_attachment_owner;
