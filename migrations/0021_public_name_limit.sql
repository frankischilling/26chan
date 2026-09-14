-- Public source checks names and subjects against 100 input bytes. Widen the
-- name storage bound without changing historical rows or runtime authority.
ALTER TABLE content.posts DROP CONSTRAINT posts_name_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_name_check
    CHECK (octet_length(name) BETWEEN 1 AND 100);
-- Keep the 120-byte subject storage ceiling for historical posts. New writes
-- through the application enforce 100 bytes; tightening this table constraint
-- would invalidate older rows or prevent their deletion/moderation updates.
