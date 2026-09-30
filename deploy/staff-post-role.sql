-- Existing installations: bootstrap administrator, before migration 0042.
CREATE ROLE board_staff_post_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_staff_post_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
