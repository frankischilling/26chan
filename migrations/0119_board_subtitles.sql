-- Audited source SUBTITLE profiles; descriptions and existing policy stay unchanged.
-- Only fixed template markup may render these profiles.
ALTER TABLE content.boards ADD COLUMN board_subtitle text NOT NULL DEFAULT 'none'
    CONSTRAINT boards_subtitle_profile CHECK (board_subtitle IN ('none','fiction','worksafe_gif'));
UPDATE content.boards b SET board_subtitle=policy.profile FROM (VALUES
('gif','worksafe_gif'),
('b','fiction'),
('trash','fiction')) policy(slug,profile) WHERE b.slug=policy.slug;
