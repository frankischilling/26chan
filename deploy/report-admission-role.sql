-- Existing installations: bootstrap administrator, before migration 0094.
CREATE ROLE board_report_admission_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_report_admission_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
