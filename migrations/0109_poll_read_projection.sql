-- Partial read-side projection for polls.tpl.php:21-32 and
-- polls-view.tpl.php:19-60. No source controller/schema was supplied: these
-- limits and integer scores are rewrite safety choices, not recovered voting
-- rules, score precision, catalogue selection, or publication semantics.
-- Operator-owned configuration only; no runtime writer or production seeds.
-- /polls/{id} is a global page namespace. Refuse an incompatible existing
-- board atomically; never rename or delete operator data to make room.
ALTER TABLE content.boards ADD CONSTRAINT boards_reserved_polls_route
    CHECK (slug <> 'polls');

CREATE SCHEMA poll_private AUTHORIZATION board_migrator;
REVOKE ALL ON SCHEMA poll_private FROM PUBLIC,board_public,board_staff,board_auth;

CREATE TABLE poll_private.polls (
    id bigint PRIMARY KEY CHECK (id > 0),
    title text NOT NULL CHECK (octet_length(title) <= 512),
    description text NOT NULL CHECK (octet_length(description) <= 16384),
    vote_count bigint NOT NULL CHECK (vote_count BETWEEN 0 AND 1000000000),
    published boolean NOT NULL DEFAULT false,
    catalogue_ordinal integer UNIQUE CHECK (catalogue_ordinal BETWEEN 1 AND 200)
);
CREATE TABLE poll_private.options (
    poll_id bigint NOT NULL REFERENCES poll_private.polls(id) ON DELETE CASCADE,
    id bigint NOT NULL CHECK (id > 0),
    ordinal integer NOT NULL CHECK (ordinal BETWEEN 1 AND 128),
    caption text NOT NULL CHECK (octet_length(caption) <= 1024),
    score bigint CHECK (score BETWEEN 0 AND 1000000000),
    PRIMARY KEY (poll_id,id),
    UNIQUE (poll_id,ordinal)
);
REVOKE ALL ON poll_private.polls,poll_private.options
    FROM PUBLIC,board_public,board_staff,board_auth;

CREATE VIEW content.published_polls WITH (security_barrier=true) AS
    SELECT id,title,description,vote_count,catalogue_ordinal
    FROM poll_private.polls WHERE published;
-- Apply publication filtering here too: directly selecting this view cannot
-- reveal options for an unpublished poll. Listing is independent of detail.
CREATE VIEW content.published_poll_options WITH (security_barrier=true) AS
    SELECT o.poll_id,o.id,o.ordinal,o.caption,o.score
    FROM poll_private.options o JOIN poll_private.polls p ON p.id=o.poll_id
    WHERE p.published;
REVOKE ALL ON content.published_polls,content.published_poll_options
    FROM PUBLIC,board_public,board_staff,board_auth;
GRANT SELECT ON content.published_polls,content.published_poll_options TO board_public;
