-- File MD5 describes the approved normalized bytes (E-008). Keep the existing
-- review metadata and append only a digest that is safe to open right now.
CREATE OR REPLACE VIEW content.staff_post_media WITH (security_barrier = true) AS
    SELECT m.post_id,m.filename,m.bytes,m.width,m.height,m.spoiler,m.tim,
        a.thumbnail_width,a.thumbnail_height,
        (NOT m.file_deleted AND NOT p.deleted AND a.id IS NOT NULL
         AND EXISTS (SELECT 1 FROM content.visible_threads t
                     WHERE t.id=p.thread_id AND t.board=p.board)) AS available,
        CASE WHEN NOT m.file_deleted AND NOT p.deleted AND a.id IS NOT NULL
             AND EXISTS (SELECT 1 FROM content.visible_threads t
                         WHERE t.id=p.thread_id AND t.board=p.board
                           AND (t.archived_at IS NULL
                                OR t.archive_expires_at>clock_timestamp()))
             AND a.md5 ~ '^[0-9a-f]{32}$' AND octet_length(a.md5)=32
             THEN a.md5 END AS md5
    FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
    LEFT JOIN media.assets a ON a.id=m.asset_id AND a.state='approved';
