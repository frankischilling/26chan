-- Existing installations: bootstrap administrator, before migration 0040.
CREATE ROLE board_poster_count_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_poster_count_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
