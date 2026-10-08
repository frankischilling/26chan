-- Force archive uses the existing staff thread privileges and archive triggers.
-- Extend only the audit action vocabulary and version-1 snapshot eligibility.
-- Preserve historical NULL snapshots, field bounds, randomizer checks and masks.
ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK(action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler',
        'undead','unundead','thread-options','force-archive'));

ALTER TABLE content.moderation_audit
    DROP CONSTRAINT moderation_audit_snapshot_shape,
    ADD CONSTRAINT moderation_audit_snapshot_shape CHECK ((
        (snapshot_version IS NULL
         AND snapshot_name IS NULL AND snapshot_trip IS NULL AND snapshot_capcode IS NULL
         AND snapshot_subject IS NULL AND snapshot_comment IS NULL
         AND snapshot_comment_format IS NULL AND snapshot_staff_authorized_limits IS NULL
         AND snapshot_wordfiltered IS NULL AND snapshot_image_spoiler IS NULL
         AND snapshot_filename IS NULL AND snapshot_dice_result IS NULL
         AND snapshot_fortune_text IS NULL AND snapshot_fortune_color IS NULL)
        OR
        (snapshot_version = 1 AND action IN ('thread-options','spoiler','unspoiler','force-archive')
         AND snapshot_name IS NOT NULL AND snapshot_subject IS NOT NULL
         AND snapshot_comment IS NOT NULL AND snapshot_comment_format IS NOT NULL
         AND snapshot_staff_authorized_limits IS NOT NULL
         AND snapshot_wordfiltered IS NOT NULL AND snapshot_image_spoiler IS NOT NULL)
    ) IS TRUE);
