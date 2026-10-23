-- Add SWF manifests under the existing authority preflights.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before SWF manifest'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before SWF manifest'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before SWF manifest'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before SWF manifest'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before SWF manifest'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before SWF manifest'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;
-- SWF reservations preserve original bytes and have no thumbnail or mobile derivative.
ALTER TABLE media.assets
 DROP CONSTRAINT assets_output_format_check,
 ADD CONSTRAINT assets_output_format_check CHECK(output_format IN ('png','jpeg','gif','pdf','webm','mp4','swf')),
 DROP CONSTRAINT assets_bytes_check,
 ADD CONSTRAINT assets_bytes_check CHECK(bytes BETWEEN (CASE WHEN output_format='swf' THEN 8 ELSE 1 END) AND CASE
  WHEN output_format='swf' THEN 10485760
  WHEN still_profile='gif-v7' THEN 8388608
  WHEN still_profile='still-v6' THEN 10485760
  WHEN output_format='gif' THEN 20971520
  WHEN output_format IN ('pdf','webm','mp4') THEN 8388608 ELSE 5242880 END),
 DROP CONSTRAINT assets_width_check, DROP CONSTRAINT assets_height_check,
 ADD CONSTRAINT assets_width_check CHECK(width BETWEEN 1 AND CASE
  WHEN output_format='swf' THEN 107374182
  WHEN still_profile IN ('still-v6','gif-v7') THEN 10000
  WHEN output_format IN ('webm','mp4') THEN 2048 ELSE 1024 END),
 ADD CONSTRAINT assets_height_check CHECK(height BETWEEN 1 AND CASE
  WHEN output_format='swf' THEN 107374182
  WHEN still_profile IN ('still-v6','gif-v7') THEN 10000
  WHEN output_format IN ('webm','mp4') THEN 2048 ELSE 1024 END),
 DROP CONSTRAINT normalized_manifest,
 ADD CONSTRAINT normalized_manifest CHECK (
  (output_format='swf' AND md5 IS NOT NULL AND md5 ~ '^[0-9a-f]{32}$' AND octet_length(md5)=32
   AND thumbnail_sha256 IS NULL AND thumbnail_bytes IS NULL
   AND thumbnail_width IS NULL AND thumbnail_height IS NULL
   AND still_profile IS NULL AND source_profile IS NULL)
  OR (output_format<>'swf' AND (
   (md5 IS NULL AND thumbnail_sha256 IS NULL AND thumbnail_bytes IS NULL
    AND thumbnail_width IS NULL AND thumbnail_height IS NULL)
   OR (md5 IS NOT NULL AND md5 ~ '^[0-9a-f]{32}$' AND octet_length(md5)=32
    AND thumbnail_sha256 IS NOT NULL AND thumbnail_sha256 ~ '^[0-9a-f]{64}$' AND octet_length(thumbnail_sha256)=64
    AND thumbnail_bytes IS NOT NULL AND thumbnail_bytes BETWEEN 1 AND 5242880
    AND thumbnail_width IS NOT NULL AND thumbnail_width BETWEEN 1 AND 250
    AND (output_format IN ('pdf','webm','mp4') OR thumbnail_width<=width)
    AND thumbnail_height IS NOT NULL AND thumbnail_height BETWEEN 1 AND 250
    AND (output_format IN ('pdf','webm','mp4') OR thumbnail_height<=height)))));

-- The trigger has no callable publication API. Runtime table grants stay unchanged.
-- The migration owner can inspect private input receipts without exposing them
-- to the HTTP reader or granting the publisher direct receipt-table access.
CREATE FUNCTION media.guard_swf_manifest() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.output_format<>'swf' THEN RETURN NEW; END IF;
 IF TG_OP='INSERT' OR (NEW.state='approved' AND OLD.state IS DISTINCT FROM 'approved') THEN
  PERFORM 1 FROM media.jobs WHERE id=NEW.job_id FOR UPDATE;
  IF (TG_OP='INSERT' AND NEW.state<>'pending') OR NOT EXISTS (
   SELECT 1 FROM media.jobs j JOIN media.image_context c ON c.job_id=j.id
    JOIN content.boards b ON b.slug=c.board
   WHERE j.id=NEW.job_id AND j.input_kind='image-v1' AND j.lease_token=NEW.lease_token
    AND ((TG_OP='INSERT' AND j.state='processing' AND j.expires_at>clock_timestamp())
      OR (TG_OP='UPDATE' AND j.state='published' AND j.output_sha256=NEW.sha256 AND j.output_bytes=NEW.bytes))
    AND j.input_bytes=NEW.bytes AND c.thread_id=0 AND b.upload_board AND NOT b.staff_only
  ) THEN RAISE EXCEPTION 'SWF requires an upload-board opening-post lease.' USING ERRCODE='42501'; END IF;
 END IF;
 IF NEW.state='approved' AND NOT EXISTS (
  SELECT 1 FROM media.input_receipts r WHERE r.asset_id=NEW.id AND r.component='upload'
   AND r.sha256=NEW.sha256 AND r.bytes=NEW.bytes AND encode(r.md5,'hex')=NEW.md5
 ) THEN RAISE EXCEPTION 'SWF original-byte receipt is unavailable.' USING ERRCODE='42501'; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_swf_manifest() FROM PUBLIC;
CREATE TRIGGER media_swf_manifest_guard BEFORE INSERT OR UPDATE ON media.assets
 FOR EACH ROW EXECUTE FUNCTION media.guard_swf_manifest();

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
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF manifest changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF manifest changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF manifest changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF manifest changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF manifest changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF manifest changed challenge authority'; END IF; END $$;
RESET ROLE;
