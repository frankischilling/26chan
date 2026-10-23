-- Stored April 2024 presentation values are not Pass purchase years.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before optional event authority'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM event_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted before score scopes'; END IF;
END $$;
RESET ROLE;
-- Preserve public Pass years and the source's 18 stored stock identities.
-- The public challenge trigger still clears caller-supplied values and only
-- stamps verified Pass years. This migration imports no identities or events.
ALTER TABLE content.posts DROP CONSTRAINT posts_since4pass_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_since4pass_check CHECK(since4pass>=0 AND since4pass<=10017);
SET LOCAL ROLE board_event_owner;
SELECT set_config('board.migration_score_event_hash',encode(event_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE event_private.catalog SET definition_hash=decode(current_setting('board.migration_score_event_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_image_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_image_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_image_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_image_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN IF content.events_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Stock presentation migration changed event authority'; END IF; END $$;
RESET ROLE;
