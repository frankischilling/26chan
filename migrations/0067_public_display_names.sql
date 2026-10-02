-- Source accepts at most 100 raw public name bytes, then checks escaped name
-- and trip markup against 255 bytes. Tab expansion can widen stored plain text;
-- a trip-only name is empty. Keep escaped markup out of the name column.
-- No historical row, clock, identity, policy or runtime grant is changed.
ALTER TABLE content.posts DROP CONSTRAINT posts_name_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_name_check
    CHECK (octet_length(name) <= 255);
