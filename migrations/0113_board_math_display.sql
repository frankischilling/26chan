-- Pinned JSMATH display policy: disabled globally, enabled only on /sci/.
-- Math is a browser projection; stored comments and API delimiters stay literal.
ALTER TABLE content.boards ADD COLUMN math_tags boolean NOT NULL DEFAULT false;
UPDATE content.boards SET math_tags=true WHERE slug='sci';
