-- Staff can review display metadata after removal, but cannot read the raw
-- attachment table, upload capabilities, or media publication authority.
CREATE VIEW content.staff_post_media WITH (security_barrier = true) AS
    SELECT m.post_id,m.filename,m.bytes,m.width,m.height,m.spoiler,m.tim,
        a.thumbnail_width,a.thumbnail_height,
        (NOT m.file_deleted AND NOT p.deleted AND a.id IS NOT NULL
         AND EXISTS (SELECT 1 FROM content.visible_threads t
                     WHERE t.id=p.thread_id AND t.board=p.board)) AS available
    FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
    LEFT JOIN media.assets a ON a.id=m.asset_id AND a.state='approved';
REVOKE ALL ON content.staff_post_media FROM PUBLIC;
GRANT SELECT ON content.staff_post_media TO board_staff;

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK (action IN ('close','reopen','sticky','unsticky','remove-post',
                     'remove-thread','remove-file','resolve','dismiss'));
