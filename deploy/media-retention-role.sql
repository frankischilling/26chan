-- Existing installations only, once as bootstrap administrator before 0016.
-- Fresh installations create this non-login owner through roles.sql.
CREATE ROLE board_media_retention_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_media_retention_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
