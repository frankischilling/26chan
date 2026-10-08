-- Existing installations: bootstrap administrator, before migration 0087.
CREATE ROLE board_posting_cooldown_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_posting_cooldown_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
