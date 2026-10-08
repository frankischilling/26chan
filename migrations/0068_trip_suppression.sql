-- New posts follow the pinned STRIP_TRIPCODE setting; saved identities remain.
ALTER TABLE content.boards ADD COLUMN strip_tripcode boolean NOT NULL DEFAULT false;
GRANT SELECT(strip_tripcode) ON content.boards TO board_attachment_owner;
UPDATE content.boards b SET strip_tripcode=policy.enabled FROM (VALUES
('a',false),
('aco',false),
('c',false),
('d',false),
('e',false),
('f',false),
('g',false),
('gif',false),
('h',false),
('his',false),
('hr',false),
('k',false),
('m',false),
('n',false),
('o',false),
('p',false),
('r',false),
('s',false),
('t',false),
('u',false),
('v',false),
('w',false),
('wg',false),
('i',false),
('ic',false),
('cm',false),
('y',false),
('an',false),
('cgl',false),
('ck',false),
('co',false),
('fa',false),
('fit',false),
('jp',false),
('mlp',false),
('mu',false),
('po',false),
('sp',false),
('tg',false),
('toy',false),
('trv',false),
('tv',false),
('x',false),
('b',true),
('soc',false),
('r9k',false),
('test',false),
('adv',false),
('lit',false),
('int',false),
('sci',false),
('3',false),
('vp',false),
('diy',false),
('pol',false),
('hc',false),
('vg',false),
('hm',false),
('j',false),
('wsg',false),
('out',false),
('lgbt',false),
('vr',false),
('gd',false),
('s4s',true),
('biz',false),
('qa',false),
('trash',false),
('news',false),
('wsr',false),
('qst',false),
('bant',false),
('vip',false),
('vrpg',false),
('vmg',false),
('vst',false),
('vt',false),
('vm',false),
('pw',false),
('xs',false),
('asp',false),
('qb',false)) policy(slug,enabled) WHERE b.slug=policy.slug;

CREATE OR REPLACE FUNCTION content.apply_post_trip() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE v_suppressed boolean;
BEGIN
    SELECT forced_anon OR strip_tripcode INTO v_suppressed
    FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    IF v_suppressed AND NEW.name='' THEN
        NEW.name := 'Anonymous';
    END IF;
    NEW.trip := CASE WHEN v_suppressed THEN NULL
        ELSE nullif(current_setting('board.post_trip', true), '') END;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_post_trip() FROM PUBLIC;
