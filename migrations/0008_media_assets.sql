CREATE TABLE media.assets (
    id text PRIMARY KEY CHECK (octet_length(id) = 32 AND id ~ '^[0-9a-f]{32}$'),
    -- Queue metadata expires independently of durable approvals.
    job_id text NOT NULL CHECK (octet_length(job_id) = 32 AND job_id ~ '^[0-9a-f]{32}$'),
    lease_token text NOT NULL CHECK (octet_length(lease_token) = 32 AND lease_token ~ '^[0-9a-f]{32}$'),
    sha256 text NOT NULL CHECK (octet_length(sha256) = 64 AND sha256 ~ '^[0-9a-f]{64}$'),
    bytes bigint NOT NULL CHECK (bytes BETWEEN 1 AND 5242880),
    width integer NOT NULL CHECK (width BETWEEN 1 AND 1024),
    height integer NOT NULL CHECK (height BETWEEN 1 AND 1024),
    state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'approved', 'deleting')),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    approved_at timestamptz,
    UNIQUE (job_id, lease_token),
    CHECK ((state = 'approved') = (approved_at IS NOT NULL))
);
CREATE INDEX media_output_cleanup ON media.assets (created_at, id)
    WHERE state IN ('pending', 'deleting');

CREATE FUNCTION media.guard_asset_mutation() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog AS $$
BEGIN
    -- The migration owner retains authority for maintenance and fixture cleanup.
    IF current_user = 'board_media' AND OLD.state = 'approved' THEN
        RAISE EXCEPTION 'Approved media records are immutable.' USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'UPDATE' AND
       (NEW.id, NEW.job_id, NEW.lease_token, NEW.sha256, NEW.bytes, NEW.width, NEW.height, NEW.created_at)
       IS DISTINCT FROM
       (OLD.id, OLD.job_id, OLD.lease_token, OLD.sha256, OLD.bytes, OLD.width, OLD.height, OLD.created_at) THEN
        RAISE EXCEPTION 'Media reservation metadata is immutable.' USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION media.guard_asset_mutation() FROM PUBLIC;
CREATE TRIGGER media_asset_immutable BEFORE UPDATE OR DELETE ON media.assets
    FOR EACH ROW EXECUTE FUNCTION media.guard_asset_mutation();

CREATE VIEW media.approved_assets WITH (security_barrier = true) AS
    SELECT id, sha256, bytes, width, height FROM media.assets WHERE state = 'approved';
REVOKE ALL ON media.assets, media.approved_assets FROM PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON media.assets TO board_media;
GRANT USAGE ON SCHEMA media TO board_media_read;
GRANT SELECT ON media.approved_assets TO board_media_read;
