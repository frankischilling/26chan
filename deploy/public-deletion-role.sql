-- Existing installations: bootstrap administrator, before migration 0085.
CREATE ROLE board_public_deletion_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_public_deletion_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
