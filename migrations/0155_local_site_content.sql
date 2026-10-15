-- Operator-maintained local information pages and news. No external site content
-- or contact/payment identity is imported. Runtime access is read-only.
CREATE SCHEMA site_private AUTHORIZATION board_migrator;
REVOKE ALL ON SCHEMA site_private FROM PUBLIC,board_public,board_staff,board_auth;
CREATE TABLE site_private.pages (
    key text PRIMARY KEY,
    board text REFERENCES content.boards(slug) ON DELETE CASCADE,
    title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 256),
    body text NOT NULL CHECK (octet_length(body) BETWEEN 1 AND 8192),
    revision bigint NOT NULL CHECK (revision BETWEEN 1 AND 2147483647),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    published boolean NOT NULL DEFAULT true,
    CHECK ((board IS NULL AND key IN ('about','faq','rules','contact','feedback','legal','press','advertise','support','japanese','blog'))
        OR (board IS NOT NULL AND key='rules/'||board))
);
CREATE TABLE site_private.news (
    id bigint PRIMARY KEY CHECK (id BETWEEN 1 AND 10000),
    published_at timestamptz NOT NULL CHECK (published_at >= TIMESTAMPTZ '1970-01-01 00:00:01+00' AND published_at <= TIMESTAMPTZ '9999-12-31 23:59:59+00'),
    title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 256),
    body text NOT NULL CHECK (octet_length(body) BETWEEN 1 AND 16384),
    author text NOT NULL DEFAULT '' CHECK (octet_length(author)<=128),
    published boolean NOT NULL DEFAULT true
);
REVOKE ALL ON site_private.pages,site_private.news FROM PUBLIC,board_public,board_staff,board_auth;
CREATE VIEW content.published_site_pages WITH (security_barrier=true) AS
    SELECT p.key,p.board,p.title,p.body,p.revision,p.updated_at FROM site_private.pages p
    WHERE p.published AND (p.board IS NULL OR EXISTS (
        SELECT 1 FROM content.boards b WHERE b.slug=p.board AND NOT b.staff_only));
CREATE VIEW content.published_news WITH (security_barrier=true) AS
    SELECT id,published_at,title,body,author FROM site_private.news
    WHERE published AND published_at<=CURRENT_TIMESTAMP;
REVOKE ALL ON content.published_site_pages,content.published_news FROM PUBLIC,board_public,board_staff,board_auth;
GRANT SELECT ON content.published_site_pages,content.published_news TO board_public;
