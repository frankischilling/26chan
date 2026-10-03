ALTER TABLE content.boards
    ADD COLUMN robot9000 boolean NOT NULL DEFAULT false,
    ADD COLUMN robot9000_state_limit integer NOT NULL DEFAULT 100000
        CHECK (robot9000_state_limit BETWEEN 1 AND 10000000);
UPDATE content.boards SET robot9000=true WHERE slug='r9k';

-- Private state deliberately has no post foreign key: deleting a post does
-- not make its text original again. Actors are keyed board-specific digests.
CREATE TABLE post_secrets.robot9000_texts (
    board text NOT NULL REFERENCES content.boards(slug) ON DELETE CASCADE,
    digest bytea NOT NULL CHECK (octet_length(digest)=32),
    seen_at timestamptz NOT NULL,
    PRIMARY KEY (board,digest)
);
CREATE INDEX robot9000_texts_age ON post_secrets.robot9000_texts(board,seen_at);
CREATE TABLE post_secrets.robot9000_mutes (
    board text NOT NULL REFERENCES content.boards(slug) ON DELETE CASCADE,
    actor bytea NOT NULL CHECK (octet_length(actor)=32),
    timeout_power smallint NOT NULL CHECK (timeout_power BETWEEN 0 AND 24),
    mute_until timestamptz NOT NULL,
    next_expire timestamptz NOT NULL,
    PRIMARY KEY (board,actor)
);
REVOKE ALL ON post_secrets.robot9000_texts,post_secrets.robot9000_mutes FROM PUBLIC;
GRANT USAGE ON SCHEMA content,post_secrets TO board_robot9000_owner;
GRANT SELECT(slug,robot9000,robot9000_state_limit,staff_only),UPDATE(slug)
    ON content.boards TO board_robot9000_owner;
GRANT SELECT,INSERT,UPDATE ON post_secrets.robot9000_texts,
    post_secrets.robot9000_mutes TO board_robot9000_owner;

CREATE FUNCTION content.check_robot9000(
    p_board text,p_actor bytea,p_digest bytea,p_signal double precision,p_time timestamptz
) RETURNS TABLE(kind text,power smallint,seconds bigint,until_time timestamptz)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_enabled boolean; v_limit integer;
    v_mute post_secrets.robot9000_mutes%ROWTYPE;
    v_had_mute boolean; v_power integer; v_reason text; v_duration bigint;
BEGIN
    IF p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
       OR p_actor IS NULL OR octet_length(p_actor)<>32
       OR p_digest IS NULL OR octet_length(p_digest)<>32
       OR (p_signal IS NOT NULL AND NOT (p_signal>=0 AND p_signal<10))
       OR p_time IS NULL OR p_time<>date_trunc('second',p_time)
       OR abs(extract(epoch FROM p_time-clock_timestamp()))>30 THEN
        RAISE EXCEPTION 'Invalid Robot9000 context.' USING ERRCODE='23514';
    END IF;
    SELECT b.robot9000,b.robot9000_state_limit INTO v_enabled,v_limit
        FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
    IF NOT FOUND OR NOT v_enabled THEN
        RAISE EXCEPTION 'Robot9000 is unavailable on this board.' USING ERRCODE='23514';
    END IF;
    SELECT m.* INTO v_mute FROM post_secrets.robot9000_mutes m
        WHERE m.board=p_board AND m.actor=p_actor;
    v_had_mute := FOUND;
    v_power := CASE WHEN v_had_mute THEN v_mute.timeout_power ELSE 0 END;
    IF v_had_mute AND v_mute.mute_until>p_time THEN
        RETURN QUERY SELECT 'muted'::text,v_power::smallint,
            extract(epoch FROM v_mute.mute_until-p_time)::bigint,v_mute.mute_until;
        RETURN;
    END IF;
    IF p_signal IS NOT NULL THEN
        v_reason := 'low_signal';
    ELSIF EXISTS (SELECT 1 FROM post_secrets.robot9000_texts h
                  WHERE h.board=p_board AND h.digest=p_digest) THEN
        UPDATE post_secrets.robot9000_texts h SET seen_at=p_time
            WHERE h.board=p_board AND h.digest=p_digest;
        v_reason := 'duplicate';
    ELSE
        IF (SELECT count(*) FROM post_secrets.robot9000_texts h WHERE h.board=p_board)>=v_limit THEN
            RAISE EXCEPTION 'Robot9000 history capacity is exhausted.' USING ERRCODE='53300';
        END IF;
        INSERT INTO post_secrets.robot9000_texts(board,digest,seen_at)
            VALUES(p_board,p_digest,p_time);
    END IF;
    IF v_reason IS NOT NULL THEN
        IF NOT v_had_mute AND (SELECT count(*) FROM post_secrets.robot9000_mutes m WHERE m.board=p_board)>=v_limit THEN
            RAISE EXCEPTION 'Robot9000 mute capacity is exhausted.' USING ERRCODE='53300';
        END IF;
        v_duration := least((1::bigint << (v_power+1)),31536000::bigint);
        IF v_power<24 THEN v_power:=v_power+1; END IF;
        INSERT INTO post_secrets.robot9000_mutes(board,actor,timeout_power,mute_until,next_expire)
            VALUES(p_board,p_actor,v_power,p_time+make_interval(secs=>v_duration),
                   p_time+make_interval(secs=>v_duration))
            ON CONFLICT (board,actor) DO UPDATE SET timeout_power=EXCLUDED.timeout_power,
                mute_until=EXCLUDED.mute_until,next_expire=EXCLUDED.next_expire;
        RETURN QUERY SELECT v_reason,v_power::smallint,v_duration,
            p_time+make_interval(secs=>v_duration);
        RETURN;
    END IF;
    -- The source decays one level on a successful post, not once for every
    -- elapsed day. Its strict expiry comparison is retained.
    IF v_had_mute AND v_mute.next_expire<p_time THEN
        UPDATE post_secrets.robot9000_mutes m SET timeout_power=greatest(v_power-1,0),
            next_expire=p_time+interval '1 day' WHERE m.board=p_board AND m.actor=p_actor;
    END IF;
    RETURN QUERY SELECT 'allow'::text,0::smallint,0::bigint,NULL::timestamptz;
END $$;
REVOKE ALL ON FUNCTION content.check_robot9000(text,bytea,bytea,double precision,timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.check_robot9000(text,bytea,bytea,double precision,timestamptz)
    TO board_public;
GRANT CREATE ON SCHEMA content TO board_robot9000_owner;
ALTER FUNCTION content.check_robot9000(text,bytea,bytea,double precision,timestamptz)
    OWNER TO board_robot9000_owner;
REVOKE CREATE ON SCHEMA content FROM board_robot9000_owner;
