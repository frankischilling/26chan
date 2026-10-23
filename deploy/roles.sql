-- Run once as the database bootstrap administrator, outside application runtimes.
-- Supply passwords through your secret manager or an interactive psql session.
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
CREATE ROLE board_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_public LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_staff NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_auth NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_media LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_media_read NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_monitor NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_media_intake NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_media_intake_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_media_intake_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_attachment_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_attachment_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_poster_count_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_poster_count_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_staff_post_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_staff_post_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_media_retention_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_media_retention_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_robot9000_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_robot9000_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_anonymous_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_anonymous_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_admission_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_admission_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_public_deletion_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_public_deletion_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
ALTER ROLE board_media_intake SET statement_timeout = '5s';
ALTER ROLE board_media_intake SET lock_timeout = '2s';
ALTER ROLE board_media_intake SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_media_intake SET search_path = pg_catalog;
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
ALTER ROLE board_monitor SET statement_timeout = '2s';
ALTER ROLE board_monitor SET lock_timeout = '1s';
ALTER ROLE board_monitor SET idle_in_transaction_session_timeout = '2s';
ALTER ROLE board_monitor SET search_path = pg_catalog;
CREATE ROLE board_posting_cooldown_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_posting_cooldown_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_report_admission_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_report_admission_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
CREATE ROLE board_poll_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_poll_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
-- Run as the bootstrap administrator before migration 0129 on an existing database.
CREATE ROLE board_content_erasure_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_content_erasure_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
-- Bootstrap before migration 0131 on existing installations. No runtime membership.
CREATE ROLE board_thread_transfer_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_thread_transfer_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
-- Bootstrap before migration 0133; application roles cannot assume this identity.
CREATE ROLE board_report_snapshot_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_report_snapshot_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
-- Bootstrap before migration 0134. Neither application can assume this owner.
CREATE ROLE board_restriction_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_restriction_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
-- Bootstrap before 0142. Configure the worker login outside application runtimes.
CREATE ROLE board_moderation_retention NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE ROLE board_moderation_retention_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_moderation_retention_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
ALTER ROLE board_moderation_retention SET statement_timeout='5s';
ALTER ROLE board_moderation_retention SET lock_timeout='2s';
ALTER ROLE board_moderation_retention SET idle_in_transaction_session_timeout='5s';
ALTER ROLE board_moderation_retention SET search_path=pg_catalog;
-- Bootstrap before 0234; only authenticated payment adapters use this runtime.
CREATE ROLE board_checkout NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE board_checkout SET statement_timeout='5s';
ALTER ROLE board_checkout SET lock_timeout='2s';
ALTER ROLE board_checkout SET idle_in_transaction_session_timeout='5s';
ALTER ROLE board_checkout SET search_path=pg_catalog;
-- Bootstrap before 0251. Event state has no runtime table grants or login owner.
CREATE ROLE board_event_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_event_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
-- Bootstrap before 0258. Submission identities remain outside public/staff reads.
CREATE ROLE board_submission_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_submission_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
-- Bootstrap before 0262. Feedback review has no public write authority.
CREATE ROLE board_submission_review_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_submission_review_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
-- Bootstrap before 0263. Applicant identity is isolated from feedback review.
CREATE ROLE board_application_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_application_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
