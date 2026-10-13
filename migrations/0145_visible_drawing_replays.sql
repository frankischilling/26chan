-- Expose only a boolean on already-visible attachment rows. Public credentials
-- do not gain access to the private canonical-replay metadata table.
CREATE OR REPLACE VIEW content.visible_post_media WITH (security_barrier=true) AS
    SELECT m.post_id,m.asset_id,m.filename,m.bytes,m.width,m.height,m.spoiler,
        (p.file_deleted OR m.file_deleted OR a.id IS NULL) AS file_deleted,
        m.tim,encode(decode(a.md5,'hex'),'base64') AS md5,
        a.thumbnail_width,a.thumbnail_height,coalesce(a.output_format,'png') AS output_format,
        (NOT p.file_deleted AND NOT m.file_deleted AND a.id IS NOT NULL
         AND EXISTS(SELECT 1 FROM media.approved_post_replays r
             WHERE r.id=a.id AND r.board=p.board AND r.tim=m.tim)) AS has_replay
    FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
    JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board
    LEFT JOIN media.assets a ON a.id=m.asset_id AND a.state='approved'
    WHERE NOT p.deleted;
