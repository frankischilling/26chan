DO $$
BEGIN
    IF current_setting('server_encoding') <> 'UTF8' THEN
        RAISE EXCEPTION 'Migration 0007 requires UTF8 database encoding.';
    END IF;
END
$$;

ALTER TABLE content.boards RENAME COLUMN max_comment_bytes TO max_comment_chars;
ALTER TABLE content.boards RENAME CONSTRAINT boards_max_comment_bytes_check TO boards_max_comment_chars_check;

ALTER TABLE content.posts DROP CONSTRAINT posts_comment_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_check
    CHECK (char_length(comment) BETWEEN 1 AND 16000 AND octet_length(comment) <= 64000);
