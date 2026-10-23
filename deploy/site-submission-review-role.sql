-- Run as the bootstrap administrator before 0262 on an existing installation.
CREATE ROLE board_submission_review_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_submission_review_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
