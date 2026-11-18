-- Run as the bootstrap administrator before 0258 on an existing installation.
-- No application login may assume this role.
CREATE ROLE board_submission_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_submission_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
