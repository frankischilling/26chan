-- Run as the bootstrap administrator before migration 0123 on an existing database.
CREATE ROLE board_content_erasure_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_content_erasure_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
