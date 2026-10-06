-- Pinned source: imgboard.php:2851-2869 and global_config.ini:281.
-- EXPIRE_NEGLECTED defaults to root/bump-clock order; /f/ alone overrides
-- it to OP-number order (config/boards/f.config.ini:21-23).
-- /j/ inherits yes, but JANITOR_BOARD bypasses source trim_db entirely.
-- Add policy only: no post/thread clocks, content, retention or ACL changes.
ALTER TABLE content.boards
    ADD COLUMN expire_neglected boolean NOT NULL DEFAULT true;
UPDATE content.boards SET expire_neglected=false WHERE slug='f';
-- Existing runtime roles can read policy but receive no policy-write authority.
