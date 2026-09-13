-- Existing installations only, once as the bootstrap administrator before 0012.
-- Fresh installations already create this role through roles.sql.
CREATE ROLE board_attachment_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_attachment_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
