-- Companion metadata follows the existing approved-file deletion and visibility gate.
CREATE OR REPLACE VIEW media.approved_assets WITH (security_barrier=true) AS
    SELECT a.id,a.sha256,a.bytes,a.width,a.height,
        a.thumbnail_sha256,a.thumbnail_bytes,a.thumbnail_width,a.thumbnail_height,
        a.output_format,a.thumbnail_format,a.mobile_sha256,a.mobile_bytes,a.mobile_width,a.mobile_height
    FROM media.assets a WHERE a.state='approved' AND NOT EXISTS (
        SELECT 1 FROM content.post_media m WHERE m.asset_id=a.id AND (
            m.file_deleted OR NOT EXISTS (
                SELECT 1 FROM content.posts p JOIN content.visible_threads t
                    ON t.id=p.thread_id AND t.board=p.board WHERE p.id=m.post_id AND NOT p.deleted AND NOT p.file_deleted
            )
        )
    );
CREATE OR REPLACE VIEW media.approved_post_assets WITH (security_barrier=true) AS
    SELECT p.board,m.tim,a.* FROM content.post_media m
    JOIN content.posts p ON p.id=m.post_id
    JOIN media.approved_assets a ON a.id=m.asset_id;

