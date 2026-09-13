-- This view discloses only output/job IDs already visible to the coordinator.
-- Content and upload capabilities remain inaccessible to that runtime.
CREATE VIEW media.retirable_outputs WITH (security_barrier=true) AS
    SELECT a.id,a.job_id,a.approved_at FROM media.assets a
    LEFT JOIN content.post_media m ON m.asset_id=a.id
    WHERE a.state='approved' AND (
        (m.asset_id IS NOT NULL AND (m.file_deleted OR NOT EXISTS (
            SELECT 1 FROM content.posts p JOIN content.visible_threads t
                ON t.id=p.thread_id AND t.board=p.board
                WHERE p.id=m.post_id AND NOT p.deleted
        )))
        OR (m.asset_id IS NULL AND a.approved_at < clock_timestamp()-interval '1 day'
            AND NOT EXISTS (
                SELECT 1 FROM media.jobs j JOIN media_intake.handles h ON h.job_id=j.id
                WHERE j.id=a.job_id AND j.created_at+interval '2 hours'>clock_timestamp()
            ))
    );
REVOKE ALL ON media.retirable_outputs FROM PUBLIC;
GRANT SELECT ON media.retirable_outputs TO board_media,board_media_retention_owner;
GRANT USAGE ON SCHEMA media TO board_media_retention_owner;
GRANT SELECT(id,job_id,state),UPDATE(state,approved_at,updated_at) ON media.assets TO board_media_retention_owner;
GRANT SELECT(id),UPDATE(id) ON media.jobs TO board_media_retention_owner;

CREATE FUNCTION media.retire_output(p_id text) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_job text; v_changed bigint;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Output retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF p_id IS NULL OR octet_length(p_id)<>32 OR p_id !~ '^[0-9a-f]{32}$' THEN
        RAISE EXCEPTION 'Invalid media identifier.' USING ERRCODE='22023';
    END IF;
    SELECT a.job_id INTO v_job FROM media.assets a WHERE a.id=p_id;
    IF v_job IS NULL THEN RETURN false; END IF;
    -- Posting holds this same job lock through capability consumption. Take no
    -- board lock here: a poster already holds board/thread locks before the job.
    PERFORM j.id FROM media.jobs j WHERE j.id=v_job FOR UPDATE;
    PERFORM a.id FROM media.assets a WHERE a.id=p_id FOR UPDATE;
    -- A separate statement after the locks sees a poster that committed while
    -- we waited. An absent job cannot authorize a subsequent attachment.
    UPDATE media.assets a SET state='deleting',approved_at=NULL,updated_at=clock_timestamp()
        WHERE a.id=p_id AND a.state='approved'
        AND EXISTS (SELECT 1 FROM media.retirable_outputs r WHERE r.id=a.id);
    GET DIAGNOSTICS v_changed=ROW_COUNT;
    RETURN v_changed=1;
END $$;
REVOKE ALL ON FUNCTION media.retire_output(text) FROM PUBLIC;
GRANT CREATE ON SCHEMA media TO board_media_retention_owner;
ALTER FUNCTION media.retire_output(text) OWNER TO board_media_retention_owner;
REVOKE CREATE ON SCHEMA media FROM board_media_retention_owner;
SET LOCAL ROLE board_media_retention_owner;
GRANT EXECUTE ON FUNCTION media.retire_output(text) TO board_media;
RESET ROLE;

-- A deleting manifest cannot be approved again by a coordinator after cleanup
-- has committed its retirement. Keep the original metadata guard intact.
CREATE FUNCTION media.guard_retired_output() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
    IF current_user='board_media' AND OLD.state='deleting' AND NEW.state<>'deleting' THEN
        RAISE EXCEPTION 'Retired media cannot be republished.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_retired_output() FROM PUBLIC;
CREATE TRIGGER media_retired_immutable BEFORE UPDATE ON media.assets
    FOR EACH ROW EXECUTE FUNCTION media.guard_retired_output();
