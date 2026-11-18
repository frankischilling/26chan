-- Run as the bootstrap administrator before 0264 on an existing installation.
CREATE ROLE board_contest_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_contest_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
