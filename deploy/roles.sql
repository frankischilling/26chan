-- Run once as the database bootstrap administrator, outside application runtimes.
-- Supply passwords through your secret manager or an interactive psql session.
CREATE ROLE board_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_public LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_staff NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_auth NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_media LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_media_read NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE board_public SET statement_timeout = '5s';
ALTER ROLE board_public SET lock_timeout = '2s';
ALTER ROLE board_public SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_public SET search_path = pg_catalog;
ALTER ROLE board_media SET statement_timeout = '5s';
ALTER ROLE board_media SET lock_timeout = '2s';
ALTER ROLE board_media SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_media SET search_path = pg_catalog;
ALTER ROLE board_media_read SET statement_timeout = '5s';
ALTER ROLE board_media_read SET lock_timeout = '2s';
ALTER ROLE board_media_read SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_media_read SET search_path = pg_catalog;
