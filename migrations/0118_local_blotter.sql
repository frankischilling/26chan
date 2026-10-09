-- Source SHOW_BLOTTER policy and bounded, operator-owned local announcements.
-- Plain text replaces source trusted HTML. No source messages are imported.
ALTER TABLE content.boards ADD COLUMN show_blotter boolean NOT NULL DEFAULT true;
UPDATE content.boards SET show_blotter=false WHERE slug='j';
ALTER TABLE content.boards ADD CONSTRAINT boards_reserved_blotter_route CHECK (slug <> 'blotter');
CREATE SCHEMA blotter_private AUTHORIZATION board_migrator;
REVOKE ALL ON SCHEMA blotter_private FROM PUBLIC,board_public,board_staff,board_auth;
CREATE TABLE blotter_private.messages (
    id bigint PRIMARY KEY CHECK (id BETWEEN 1 AND 10000),
    published_at timestamptz NOT NULL CHECK (published_at >= TIMESTAMPTZ '1970-01-01 00:00:01+00' AND published_at <= TIMESTAMPTZ '9999-12-31 23:59:59+00'),
    content text NOT NULL CHECK (octet_length(content) BETWEEN 1 AND 8192),
    published boolean NOT NULL DEFAULT true
);
REVOKE ALL ON blotter_private.messages FROM PUBLIC,board_public,board_staff,board_auth;
CREATE VIEW content.published_blotter WITH (security_barrier=true) AS
    SELECT id,published_at,content FROM blotter_private.messages WHERE published;
REVOKE ALL ON content.published_blotter FROM PUBLIC,board_public,board_staff,board_auth;
GRANT SELECT ON content.published_blotter TO board_public;
