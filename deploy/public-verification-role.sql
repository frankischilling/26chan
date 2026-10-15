-- Bootstrap before migration 0158. Supply runtime credentials separately.
CREATE ROLE board_verifier LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_mailer LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE board_verifier SET statement_timeout='5s';
ALTER ROLE board_verifier SET lock_timeout='2s';
ALTER ROLE board_verifier SET idle_in_transaction_session_timeout='5s';
ALTER ROLE board_verifier SET search_path=pg_catalog;
ALTER ROLE board_mailer SET statement_timeout='5s';
ALTER ROLE board_mailer SET lock_timeout='2s';
ALTER ROLE board_mailer SET idle_in_transaction_session_timeout='5s';
ALTER ROLE board_mailer SET search_path=pg_catalog;
