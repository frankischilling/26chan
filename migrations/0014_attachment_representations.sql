-- Metadata describes exact normalized bytes. Legacy approvals remain readable;
-- NULL identifies outputs that predate the normalized manifest, never a fake hash.
ALTER TABLE media.assets
    ADD COLUMN md5 text,
    ADD COLUMN thumbnail_sha256 text,
    ADD COLUMN thumbnail_bytes bigint,
    ADD COLUMN thumbnail_width integer,
    ADD COLUMN thumbnail_height integer,
    ADD CONSTRAINT normalized_manifest CHECK (
        (md5 IS NULL AND thumbnail_sha256 IS NULL AND thumbnail_bytes IS NULL
         AND thumbnail_width IS NULL AND thumbnail_height IS NULL)
        OR (md5 IS NOT NULL AND md5 ~ '^[0-9a-f]{32}$' AND octet_length(md5)=32
         AND thumbnail_sha256 IS NOT NULL AND thumbnail_sha256 ~ '^[0-9a-f]{64}$' AND octet_length(thumbnail_sha256)=64
         AND thumbnail_bytes IS NOT NULL AND thumbnail_bytes BETWEEN 1 AND 5242880
         AND thumbnail_width IS NOT NULL AND thumbnail_width BETWEEN 1 AND 250 AND thumbnail_width <= width
         AND thumbnail_height IS NOT NULL AND thumbnail_height BETWEEN 1 AND 250 AND thumbnail_height <= height)
    );

CREATE OR REPLACE FUNCTION media.guard_asset_mutation() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog AS $$
BEGIN
    IF current_user = 'board_media' AND OLD.state = 'approved' THEN
        RAISE EXCEPTION 'Approved media records are immutable.' USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'UPDATE' AND
       (NEW.id,NEW.job_id,NEW.lease_token,NEW.sha256,NEW.bytes,NEW.width,NEW.height,NEW.created_at,
        NEW.md5,NEW.thumbnail_sha256,NEW.thumbnail_bytes,NEW.thumbnail_width,NEW.thumbnail_height)
       IS DISTINCT FROM
       (OLD.id,OLD.job_id,OLD.lease_token,OLD.sha256,OLD.bytes,OLD.width,OLD.height,OLD.created_at,
        OLD.md5,OLD.thumbnail_sha256,OLD.thumbnail_bytes,OLD.thumbnail_width,OLD.thumbnail_height) THEN
        RAISE EXCEPTION 'Media reservation metadata is immutable.' USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;

-- Monotonic millisecond media number. A row lock handles simultaneous uploads
-- and backward wall-clock movement; allocation rolls back with the post.
CREATE TABLE content.media_clock (
    singleton boolean PRIMARY KEY CHECK(singleton),
    last_number bigint NOT NULL CHECK(last_number BETWEEN 0 AND 9007199254740991)
);
INSERT INTO content.media_clock VALUES (true,0);
REVOKE ALL ON content.media_clock FROM PUBLIC;
GRANT SELECT,UPDATE(last_number) ON content.media_clock TO board_attachment_owner;
CREATE FUNCTION content.next_media_number() RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE number bigint;
BEGIN
    UPDATE content.media_clock SET last_number=greatest(last_number+1,
        floor(extract(epoch FROM clock_timestamp())*1000)::bigint)
        WHERE singleton RETURNING last_number INTO number;
    IF number IS NULL THEN RAISE EXCEPTION 'Media clock is unavailable.'; END IF;
    RETURN number;
END $$;
REVOKE ALL ON FUNCTION content.next_media_number() FROM PUBLIC;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
ALTER FUNCTION content.next_media_number() OWNER TO board_attachment_owner;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
GRANT EXECUTE ON FUNCTION content.next_media_number() TO board_migrator;
RESET ROLE;
ALTER TABLE content.post_media ADD COLUMN tim bigint NOT NULL DEFAULT content.next_media_number();
ALTER TABLE content.post_media ADD CONSTRAINT post_media_number UNIQUE(tim),
    ADD CONSTRAINT post_media_number_range CHECK(tim BETWEEN 1 AND 9007199254740991);

CREATE OR REPLACE VIEW content.visible_post_media WITH (security_barrier = true) AS
    SELECT m.post_id,m.asset_id,m.filename,m.bytes,m.width,m.height,m.spoiler,
        (m.file_deleted OR a.id IS NULL) AS file_deleted,
        m.tim,encode(decode(a.md5,'hex'),'base64') AS md5,
        a.thumbnail_width,a.thumbnail_height
    FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
    JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board
    LEFT JOIN media.assets a ON a.id=m.asset_id AND a.state='approved'
    WHERE NOT p.deleted;

CREATE OR REPLACE VIEW media.approved_assets WITH (security_barrier = true) AS
    SELECT a.id,a.sha256,a.bytes,a.width,a.height,
        a.thumbnail_sha256,a.thumbnail_bytes,a.thumbnail_width,a.thumbnail_height
    FROM media.assets a WHERE a.state='approved' AND NOT EXISTS (
        SELECT 1 FROM content.post_media m WHERE m.asset_id=a.id AND (
            m.file_deleted OR NOT EXISTS (
                SELECT 1 FROM content.posts p JOIN content.visible_threads t
                    ON t.id=p.thread_id AND t.board=p.board WHERE p.id=m.post_id AND NOT p.deleted
            )
        )
    );
CREATE VIEW media.approved_post_assets WITH (security_barrier = true) AS
    SELECT p.board,m.tim,a.* FROM content.post_media m
    JOIN content.posts p ON p.id=m.post_id
    JOIN media.approved_assets a ON a.id=m.asset_id;
REVOKE ALL ON media.approved_post_assets FROM PUBLIC;
GRANT SELECT ON media.approved_post_assets TO board_media_read;
