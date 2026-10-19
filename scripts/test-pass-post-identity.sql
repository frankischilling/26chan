-- These schema mutations are confined to rollback-only owned qualification.
BEGIN;
ALTER TABLE content.posts DISABLE TRIGGER pass_post_identity_erasure;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Disabled membership cleanup escaped attestation'; END IF; END $$;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Transfer accepted disabled membership cleanup'; END IF; END $$;
ROLLBACK;

BEGIN;
ALTER TABLE verification_private.pass_posts DISABLE TRIGGER ALL;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Disabled membership foreign-key checks escaped attestation'; END IF; END $$;
ROLLBACK;

BEGIN;
SET LOCAL ROLE board_anonymous_owner;
GRANT SELECT ON verification_private.pass_posts TO board_public;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Private membership read grant escaped attestation'; END IF; END $$;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Transfer accepted private membership disclosure'; END IF; END $$;
ROLLBACK;

SET ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Rollback did not restore private membership authority'; END IF; END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Rollback did not restore membership-aware transfer authority'; END IF; END $$;
RESET ROLE;

BEGIN;
SET LOCAL ROLE board_posting_cooldown_owner;
GRANT EXECUTE ON FUNCTION content.pass_duplicate_cooldown_core(bytea,text,bigint,bigint,bytea,boolean) TO board_public;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Direct duplicate-image discount grant escaped attestation'; END IF; END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_attachment_owner;
GRANT EXECUTE ON FUNCTION content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean) TO PUBLIC;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Attachment metadata privilege drift escaped membership attestation'; END IF; END $$;
ROLLBACK;
