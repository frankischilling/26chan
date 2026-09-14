-- These settings select source comment sanitation, not markup authority.
ALTER TABLE content.boards
    ADD COLUMN comment_code_spacing boolean NOT NULL DEFAULT false,
    ADD COLUMN comment_sjis_spacing boolean NOT NULL DEFAULT false;

-- Active overrides in the supplied configuration; future boards are configured
-- by the operator. No historical post text is rewritten.
UPDATE content.boards SET comment_code_spacing=true WHERE slug IN ('g','j','test');
UPDATE content.boards SET comment_sjis_spacing=true WHERE slug IN ('jp','vip');
