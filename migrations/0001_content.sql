REVOKE ALL ON SCHEMA public FROM PUBLIC;
CREATE SCHEMA content;
CREATE SCHEMA post_secrets;
CREATE SCHEMA staff_identity;
CREATE SCHEMA deployment;
REVOKE ALL ON SCHEMA content, post_secrets, staff_identity, deployment FROM PUBLIC;
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;

CREATE TABLE content.boards (
    slug text PRIMARY KEY CHECK (slug ~ '^[a-z0-9]{1,10}$'),
    title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 120),
    description text NOT NULL CHECK (octet_length(description) <= 2000),
    max_comment_bytes integer NOT NULL CHECK (max_comment_bytes BETWEEN 1 AND 16000),
    reply_limit integer NOT NULL CHECK (reply_limit BETWEEN 1 AND 1000),
    bump_limit integer NOT NULL CHECK (bump_limit BETWEEN 0 AND reply_limit),
    thread_limit integer NOT NULL CHECK (thread_limit BETWEEN 1 AND 1000),
    threads_per_page integer NOT NULL CHECK (threads_per_page BETWEEN 1 AND 20),
    worksafe boolean NOT NULL DEFAULT true
);
CREATE SEQUENCE content.post_number;
CREATE TABLE content.threads (
    id bigint PRIMARY KEY DEFAULT nextval('content.post_number'),
    board text NOT NULL REFERENCES content.boards(slug),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    bumped_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    modified_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    reply_count integer NOT NULL DEFAULT 0 CHECK (reply_count BETWEEN 0 AND 1000),
    sticky boolean NOT NULL DEFAULT false,
    closed boolean NOT NULL DEFAULT false,
    deleted boolean NOT NULL DEFAULT false,
    UNIQUE(board, id)
);
CREATE INDEX thread_board_order ON content.threads(board, sticky DESC, bumped_at DESC, id DESC) WHERE NOT deleted;
CREATE TABLE content.posts (
    id bigint PRIMARY KEY DEFAULT nextval('content.post_number'),
    board text NOT NULL,
    thread_id bigint NOT NULL,
    name text NOT NULL CHECK (octet_length(name) BETWEEN 1 AND 80),
    subject text NOT NULL CHECK (octet_length(subject) <= 120),
    comment text NOT NULL CHECK (octet_length(comment) BETWEEN 1 AND 16000),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    deleted boolean NOT NULL DEFAULT false,
    UNIQUE(board, id),
    FOREIGN KEY(board, thread_id) REFERENCES content.threads(board, id)
);
CREATE INDEX post_thread_order ON content.posts(board, thread_id, id) WHERE NOT deleted;
CREATE TABLE post_secrets.deletion (
    post_id bigint PRIMARY KEY REFERENCES content.posts(id),
    password_hash text NOT NULL CHECK (octet_length(password_hash) <= 256)
);
CREATE TABLE content.reports (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    board text NOT NULL,
    post_id bigint NOT NULL,
    reason text NOT NULL CHECK (octet_length(reason) BETWEEN 1 AND 1000),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    state text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'resolved', 'dismissed')),
    FOREIGN KEY(board, post_id) REFERENCES content.posts(board, id)
);
-- Reserved schemas have real grants, but staff authentication is not implemented.
CREATE TABLE staff_identity.accounts (id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, role text NOT NULL CHECK (role IN ('moderator', 'admin')), revoked_at timestamptz);
CREATE TABLE staff_identity.credentials (id bytea PRIMARY KEY, account_id bigint NOT NULL REFERENCES staff_identity.accounts(id), credential jsonb NOT NULL);
CREATE TABLE deployment.settings (key text PRIMARY KEY, value text NOT NULL);

GRANT USAGE ON SCHEMA content, post_secrets TO board_public;
GRANT SELECT ON content.boards, content.threads, content.posts TO board_public;
-- A row lock on board settings serializes the inexpensive per-board mutations.
GRANT UPDATE(slug) ON content.boards TO board_public;
GRANT INSERT(id, board) ON content.threads TO board_public;
GRANT UPDATE(reply_count, bumped_at, modified_at, deleted) ON content.threads TO board_public;
GRANT INSERT(id, board, thread_id, name, subject, comment) ON content.posts TO board_public;
GRANT UPDATE(deleted) ON content.posts TO board_public;
GRANT USAGE ON SEQUENCE content.post_number, content.reports_id_seq TO board_public;
GRANT SELECT, INSERT ON post_secrets.deletion TO board_public;
GRANT INSERT(board, post_id, reason) ON content.reports TO board_public;

GRANT USAGE ON SCHEMA content TO board_staff;
GRANT SELECT ON content.boards, content.threads, content.posts, content.reports TO board_staff;
GRANT UPDATE(sticky, closed, deleted, modified_at) ON content.threads TO board_staff;
GRANT UPDATE(deleted) ON content.posts TO board_staff;
GRANT UPDATE(state) ON content.reports TO board_staff;
GRANT USAGE ON SCHEMA staff_identity TO board_auth;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA staff_identity TO board_auth;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA staff_identity TO board_auth;
