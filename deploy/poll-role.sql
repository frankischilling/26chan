-- Existing installations: bootstrap administrator, before migration 0128.
CREATE ROLE board_poll_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_poll_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
