-- Raw public subjects remain limited to 100 input bytes in the application.
-- Permit the source's four-space tab expansion without truncation or rewriting.
ALTER TABLE content.posts DROP CONSTRAINT posts_subject_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_subject_check
    CHECK (octet_length(subject) <= 400);
