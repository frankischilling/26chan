-- Add a bounded ballot ledger behind the already-published poll projections.
-- The source poll view proves a radio-choice POST and results table; its
-- controller, database schema, deduplication rule and JS are not supplied.
-- These vote caps, cookie-digest ledger and outcomes are local security policy.
-- Historical polls remain closed and all existing counts/scores stay untouched.
ALTER TABLE poll_private.polls
    ADD COLUMN accepting_votes boolean NOT NULL DEFAULT false,
    ADD COLUMN new_vote_count integer NOT NULL DEFAULT 0,
    ADD COLUMN vote_capacity integer NOT NULL DEFAULT 10000,
    ADD CONSTRAINT polls_vote_capacity_check CHECK (vote_capacity >= 1 AND vote_capacity <= 100000),
    ADD CONSTRAINT polls_new_vote_count_check CHECK (
        new_vote_count >= 0 AND new_vote_count <= vote_capacity);

CREATE TABLE poll_private.votes (
    poll_id bigint NOT NULL REFERENCES poll_private.polls(id) ON DELETE CASCADE,
    voter_hash bytea NOT NULL CHECK (octet_length(voter_hash) = 32),
    voted_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (poll_id, voter_hash)
);
REVOKE ALL ON poll_private.votes FROM PUBLIC,board_public,board_staff,board_auth;

-- Append the new flag without changing existing projected fields or private
-- publication predicates; an unlisted published poll remains accessible by ID.
CREATE OR REPLACE VIEW content.published_polls WITH (security_barrier=true) AS
    SELECT id,title,description,vote_count,catalogue_ordinal,accepting_votes
    FROM poll_private.polls WHERE published;

-- A NOLOGIN owner executes only these narrow functions. It cannot create
-- objects, change poll publication/options, delete votes, or enumerate users.
GRANT USAGE ON SCHEMA content,poll_private TO board_poll_owner;
GRANT SELECT(id,published,accepting_votes,vote_count,new_vote_count,vote_capacity),
    UPDATE(vote_count,new_vote_count) ON poll_private.polls TO board_poll_owner;
GRANT SELECT(poll_id,id,ordinal,score),UPDATE(score)
    ON poll_private.options TO board_poll_owner;
GRANT SELECT(poll_id,voter_hash),INSERT(poll_id,voter_hash)
    ON poll_private.votes TO board_poll_owner;

-- The migrator's membership permits SET ROLE but does not inherit owner rights.
-- Create and grant each function as its actual owner, then revoke DDL authority.
GRANT CREATE ON SCHEMA content TO board_poll_owner;
SET ROLE board_poll_owner;

-- Smallint outcome codes: 0 recorded, 1 already voted, 2 closed,
-- 3 invalid option/uninitialized scores, 4 capacity reached, 5 not found.
-- Lock parent before child rows/ledger so a close or unpublish cannot race
-- with the vote. Every account tally and receipt commits or rolls back as one.
CREATE FUNCTION content.cast_poll_vote(
    p_poll bigint, p_option bigint, p_voter bytea
) RETURNS smallint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_poll record;
    v_option record;
    v_selected boolean := false;
    v_score bigint;
    v_inserted bigint;
    v_changed bigint;
BEGIN
    IF p_voter IS NULL OR octet_length(p_voter) <> 32 THEN
        RAISE EXCEPTION 'Invalid ballot identity.' USING ERRCODE='23514';
    END IF;
    IF p_poll IS NULL OR p_poll <= 0 THEN RETURN 5; END IF;

    SELECT p.published,p.accepting_votes,p.vote_count,p.new_vote_count,p.vote_capacity
    INTO v_poll FROM poll_private.polls p WHERE p.id=p_poll FOR UPDATE;
    IF NOT FOUND THEN RETURN 5; END IF;
    IF NOT v_poll.published THEN RETURN 5; END IF;

    -- A retry after closing or exhaustion still acknowledges its receipt.
    IF EXISTS (
        SELECT 1 FROM poll_private.votes v
        WHERE v.poll_id=p_poll AND v.voter_hash=p_voter
    ) THEN RETURN 1; END IF;
    IF NOT v_poll.accepting_votes THEN RETURN 2; END IF;
    IF p_option IS NULL OR p_option <= 0 THEN RETURN 3; END IF;

    -- Lock every option in a stable order. This also prevents a concurrent
    -- operator's score edit/delete from changing a checked row before update.
    -- The parent FOR UPDATE prevents new FK-checked option insertions.
    FOR v_option IN
        SELECT o.id,o.score FROM poll_private.options o
        WHERE o.poll_id=p_poll ORDER BY o.ordinal,o.id FOR UPDATE
    LOOP
        IF v_option.score IS NULL THEN RETURN 3; END IF;
        IF v_option.id=p_option THEN
            v_selected:=true;
            v_score:=v_option.score;
        END IF;
    END LOOP;
    IF NOT v_selected THEN RETURN 3; END IF;
    IF v_poll.new_vote_count >= v_poll.vote_capacity
        OR v_poll.vote_count >= 1000000000 OR v_score >= 1000000000
    THEN RETURN 4; END IF;

    INSERT INTO poll_private.votes(poll_id,voter_hash) VALUES(p_poll,p_voter)
        ON CONFLICT (poll_id,voter_hash) DO NOTHING;
    GET DIAGNOSTICS v_inserted=ROW_COUNT;
    IF v_inserted=0 THEN RETURN 1; END IF;
    UPDATE poll_private.options SET score=score+1
    WHERE poll_id=p_poll AND id=p_option AND score IS NOT NULL AND score<1000000000;
    GET DIAGNOSTICS v_changed=ROW_COUNT;
    IF v_changed<>1 THEN
        RAISE EXCEPTION 'Poll score changed during vote.' USING ERRCODE='23514';
    END IF;
    UPDATE poll_private.polls
        SET vote_count=vote_count+1,new_vote_count=new_vote_count+1
    WHERE id=p_poll AND published AND accepting_votes
        AND vote_count<1000000000 AND new_vote_count<vote_capacity;
    GET DIAGNOSTICS v_changed=ROW_COUNT;
    IF v_changed<>1 THEN
        RAISE EXCEPTION 'Poll count changed during vote.' USING ERRCODE='23514';
    END IF;
    RETURN 0;
END $$;
REVOKE ALL ON FUNCTION content.cast_poll_vote(bigint,bigint,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.cast_poll_vote(bigint,bigint,bytea) TO board_public;

-- NULL means missing or unpublished. Never disclose whether a ballot exists
-- against a hidden poll, even when given its previously accepted digest.
CREATE FUNCTION content.has_poll_vote(p_poll bigint,p_voter bytea) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF p_voter IS NULL OR octet_length(p_voter)<>32 THEN
        RAISE EXCEPTION 'Invalid ballot identity.' USING ERRCODE='23514';
    END IF;
    IF p_poll IS NULL OR p_poll<=0 OR NOT EXISTS (
        SELECT 1 FROM poll_private.polls p WHERE p.id=p_poll AND p.published
    ) THEN RETURN NULL; END IF;
    RETURN EXISTS (
        SELECT 1 FROM poll_private.votes v
        WHERE v.poll_id=p_poll AND v.voter_hash=p_voter
    );
END $$;
REVOKE ALL ON FUNCTION content.has_poll_vote(bigint,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.has_poll_vote(bigint,bytea) TO board_public;

-- The owner role needs no lasting DDL capability.
RESET ROLE;
REVOKE CREATE ON SCHEMA content,poll_private FROM board_poll_owner;
