-- Run as the bootstrap administrator before 0263 on an existing installation.
CREATE ROLE board_application_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_application_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
