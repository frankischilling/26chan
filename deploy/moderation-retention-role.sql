-- Bootstrap before 0142. Configure the worker login outside application runtimes.
CREATE ROLE board_moderation_retention NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_moderation_retention_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_moderation_retention_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
ALTER ROLE board_moderation_retention SET statement_timeout='5s';
ALTER ROLE board_moderation_retention SET lock_timeout='2s';
ALTER ROLE board_moderation_retention SET idle_in_transaction_session_timeout='5s';
ALTER ROLE board_moderation_retention SET search_path=pg_catalog;
