-- Preserve the accepted Oekaki annotation beside immutable moderation evidence.
-- Earlier audit rows were recorded before typed drawing metadata existed: keep
-- both unversioned and version-1 snapshots unchanged, with no retroactive fill.
ALTER TABLE content.moderation_audit
    ADD COLUMN snapshot_drawing_time_seconds integer
        CHECK (snapshot_drawing_time_seconds BETWEEN 1 AND 5184000),
    ADD COLUMN snapshot_drawing_source_post_id bigint
        CHECK (snapshot_drawing_source_post_id > 0),
    ADD CONSTRAINT moderation_audit_snapshot_drawing_source_requires_time
        CHECK (snapshot_drawing_source_post_id IS NULL OR snapshot_drawing_time_seconds IS NOT NULL);

ALTER TABLE content.moderation_audit
    DROP CONSTRAINT moderation_audit_snapshot_shape,
    ADD CONSTRAINT moderation_audit_snapshot_shape CHECK ((
        (snapshot_version IS NULL
         AND snapshot_name IS NULL AND snapshot_trip IS NULL AND snapshot_capcode IS NULL
         AND snapshot_subject IS NULL AND snapshot_comment IS NULL
         AND snapshot_comment_format IS NULL AND snapshot_staff_authorized_limits IS NULL
         AND snapshot_wordfiltered IS NULL AND snapshot_image_spoiler IS NULL
         AND snapshot_filename IS NULL AND snapshot_dice_result IS NULL
         AND snapshot_fortune_text IS NULL AND snapshot_fortune_color IS NULL
         AND snapshot_drawing_time_seconds IS NULL AND snapshot_drawing_source_post_id IS NULL)
        OR
        (snapshot_version IN (1, 2)
         AND action IN ('thread-options','spoiler','unspoiler','force-archive')
         AND snapshot_name IS NOT NULL AND snapshot_subject IS NOT NULL
         AND snapshot_comment IS NOT NULL AND snapshot_comment_format IS NOT NULL
         AND snapshot_staff_authorized_limits IS NOT NULL
         AND snapshot_wordfiltered IS NOT NULL AND snapshot_image_spoiler IS NOT NULL
         AND (snapshot_version = 2 OR
              (snapshot_drawing_time_seconds IS NULL AND snapshot_drawing_source_post_id IS NULL)))
    ) IS TRUE);

-- Original version-1 value/bounds/randomizer/mask constraints remain intact.
-- New fields are typed facts from the saved post, not appended comment HTML or
-- image provenance. Keep their authority identical to the older audit columns:
-- existing board_staff table-level SELECT/INSERT, no public reads or updates,
-- no new grant, trigger, ownership change or historical backfill.
