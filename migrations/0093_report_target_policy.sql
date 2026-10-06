-- global_config.ini:85 and boards/j.config.ini:15 in the pinned source.
-- This is board policy only: no report rows, categories, or grants are changed.
ALTER TABLE content.boards
    ADD COLUMN can_report_posts boolean NOT NULL DEFAULT true;
UPDATE content.boards SET can_report_posts=false WHERE slug='j';
