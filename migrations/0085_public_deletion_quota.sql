-- Cross-board deletion-request history contains keyed actor digests only.
-- Reservation and the public mutation must commit (or roll back) together.
CREATE TABLE post_secrets.public_deletion_capacity (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton)
);
INSERT INTO post_secrets.public_deletion_capacity VALUES (true);
CREATE TABLE post_secrets.public_deletion_actors (
    actor_hash bytea PRIMARY KEY CHECK (octet_length(actor_hash)=32),
    events bigint[] NOT NULL CHECK (
        cardinality(events)<=11
        AND (cardinality(events)=0 OR (array_ndims(events)=1 AND array_lower(events,1)=1))
        AND array_position(events,NULL) IS NULL
        AND 0<=ALL(events)),
    expires_at bigint NOT NULL CHECK (expires_at>=0)
);
CREATE INDEX public_deletion_actors_expiry ON post_secrets.public_deletion_actors(expires_at);
REVOKE ALL ON post_secrets.public_deletion_capacity,post_secrets.public_deletion_actors FROM PUBLIC;
GRANT USAGE ON SCHEMA content,post_secrets TO board_public_deletion_owner;
GRANT SELECT,UPDATE(singleton) ON post_secrets.public_deletion_capacity TO board_public_deletion_owner;
GRANT SELECT,INSERT,DELETE ON post_secrets.public_deletion_actors TO board_public_deletion_owner;
GRANT UPDATE(events,expires_at) ON post_secrets.public_deletion_actors TO board_public_deletion_owner;

CREATE FUNCTION content.reserve_public_deletion(p_actor bytea)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_events bigint[];
    v_now bigint;
    v_hour bigint;
    v_latest bigint;
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid public deletion actor.' USING ERRCODE='23514';
    END IF;
    SELECT a.events INTO v_events FROM post_secrets.public_deletion_actors a
        WHERE a.actor_hash=p_actor FOR UPDATE;
    IF NOT FOUND THEN
        -- New actors hold this guard until the caller's mutation transaction
        -- ends. Existing actors need only their own lock.
        PERFORM c.singleton FROM post_secrets.public_deletion_capacity c
            WHERE c.singleton FOR UPDATE;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'Public deletion capacity is unavailable.' USING ERRCODE='P0083';
        END IF;
        -- Another request may have created this actor while we waited.
        SELECT a.events INTO v_events FROM post_secrets.public_deletion_actors a
            WHERE a.actor_hash=p_actor FOR UPDATE;
        IF NOT FOUND THEN
            v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
            -- Opportunistic bounded cleanup is not a physical retention SLA.
            -- Strict inequality preserves events at the inclusive daily edge.
            DELETE FROM post_secrets.public_deletion_actors a WHERE a.actor_hash IN (
                SELECT old.actor_hash FROM post_secrets.public_deletion_actors old
                WHERE old.expires_at<v_now ORDER BY old.expires_at
                LIMIT 64 FOR UPDATE SKIP LOCKED
            );
            IF (SELECT count(*) FROM (
                SELECT 1 FROM post_secrets.public_deletion_actors LIMIT 100000
            ) bounded)>=100000 THEN
                RAISE EXCEPTION 'Public deletion capacity is exhausted.' USING ERRCODE='P0083';
            END IF;
            v_events:=ARRAY[]::bigint[];
            INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at)
                VALUES(p_actor,v_events,v_now+86400);
        END IF;
    END IF;
    -- Read the server clock after every potentially contended authority lock.
    v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
    SELECT coalesce(array_agg(e ORDER BY e),ARRAY[]::bigint[]),
        count(*) FILTER (WHERE e>=v_now-3600),greatest(v_now,max(e))
        INTO v_events,v_hour,v_latest
        FROM unnest(v_events) AS event(e) WHERE e>=v_now-86400;
    -- Source ordering and inclusive edges: three/hour, eleven/day successes.
    IF v_hour>2 THEN
        RAISE EXCEPTION 'Too many public deletions this hour.' USING ERRCODE='P0081';
    END IF;
    IF cardinality(v_events)>10 THEN
        RAISE EXCEPTION 'Too many public deletions today.' USING ERRCODE='P0082';
    END IF;
    UPDATE post_secrets.public_deletion_actors a
        SET events=array_append(v_events,v_now),expires_at=v_latest+86400
        WHERE a.actor_hash=p_actor;
END $$;
REVOKE ALL ON FUNCTION content.reserve_public_deletion(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.reserve_public_deletion(bytea) TO board_public;
-- Match the source's flood-before-target-validation error ordering without
-- charging failed requests. This unlocked snapshot is advisory only; the
-- transactional reservation above always repeats the check under its row lock.
CREATE FUNCTION content.check_public_deletion_quota(p_actor bytea)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_now bigint;
    v_hour bigint;
    v_day bigint;
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid public deletion actor.' USING ERRCODE='23514';
    END IF;
    v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
    SELECT count(*) FILTER (WHERE e>=v_now-3600),count(*) INTO v_hour,v_day
        FROM post_secrets.public_deletion_actors a
        CROSS JOIN LATERAL unnest(a.events) AS event(e)
        WHERE a.actor_hash=p_actor AND e>=v_now-86400;
    IF v_hour>2 THEN
        RAISE EXCEPTION 'Too many public deletions this hour.' USING ERRCODE='P0081';
    END IF;
    IF v_day>10 THEN
        RAISE EXCEPTION 'Too many public deletions today.' USING ERRCODE='P0082';
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.check_public_deletion_quota(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.check_public_deletion_quota(bytea) TO board_public;
GRANT CREATE ON SCHEMA content TO board_public_deletion_owner;
ALTER FUNCTION content.reserve_public_deletion(bytea) OWNER TO board_public_deletion_owner;
ALTER FUNCTION content.check_public_deletion_quota(bytea) OWNER TO board_public_deletion_owner;
REVOKE CREATE ON SCHEMA content FROM board_public_deletion_owner;
