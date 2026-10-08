-- Imported ENABLE_PAINTERJS, ENABLE_OEKAKI_REPLAYS and PAINTERJS_DIMS policy.
-- Replay-enabled boards retain their policy while replay support is unfinished.
-- The unused source OEKAKI_MIN/MAX constants are not admission limits.
ALTER TABLE content.boards
    ADD COLUMN oekaki boolean NOT NULL DEFAULT false,
    ADD COLUMN oekaki_replays boolean NOT NULL DEFAULT false,
    ADD COLUMN oekaki_width integer NOT NULL DEFAULT 400 CHECK (oekaki_width > 0),
    ADD COLUMN oekaki_height integer NOT NULL DEFAULT 400 CHECK (oekaki_height > 0);
UPDATE content.boards b SET oekaki=policy.enabled,oekaki_replays=policy.replays,
    oekaki_width=policy.width,oekaki_height=policy.height
FROM (VALUES
('i',true,true,400,400),
('qst',true,false,400,400),
('vip',true,false,400,400)) policy(slug,enabled,replays,width,height) WHERE b.slug=policy.slug;
