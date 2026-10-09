-- Source admin.php Board Cleanup prunes text history strictly older than two
-- calendar years, including when the robot is disabled. It never prunes mutes.
-- UTC is explicit: the supplied active PHP does not pin the MySQL timezone.
CREATE TABLE content.board_cleanup_audit (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id bigint NOT NULL CHECK(account_id>0),
    board text NOT NULL REFERENCES content.boards(slug),
    action text NOT NULL CHECK(action='robot9000-texts'),
    cutoff timestamptz NOT NULL,
    removed bigint NOT NULL CHECK(removed BETWEEN 0 AND 1000),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON content.board_cleanup_audit FROM PUBLIC;
GRANT INSERT(account_id,board,action,cutoff,removed) ON content.board_cleanup_audit
    TO board_robot9000_owner;
GRANT USAGE ON SEQUENCE content.board_cleanup_audit_id_seq TO board_robot9000_owner;
GRANT DELETE ON post_secrets.robot9000_texts TO board_robot9000_owner;

-- Retained history remains maintainable if its board becomes private. Only
-- this NOLOGIN owner gains board metadata/locking visibility; its existing
-- column grants and absence of post/thread rights remain unchanged. Public
-- check_robot9000 still explicitly rejects staff_only boards.
CREATE POLICY robot9000_cleanup_board_read ON content.boards
    FOR SELECT TO board_robot9000_owner USING(true);
CREATE POLICY robot9000_cleanup_board_lock ON content.boards
    FOR UPDATE TO board_robot9000_owner USING(true) WITH CHECK(true);

CREATE FUNCTION content.cleanup_robot9000(p_board text,p_account bigint)
RETURNS TABLE(removed bigint,has_more boolean,cutoff timestamptz)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_cutoff timestamptz; v_removed bigint; v_more boolean;
BEGIN
    IF p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
       OR p_account IS NULL OR p_account<=0
       OR current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Invalid Robot9000 cleanup context.' USING ERRCODE='22023';
    END IF;
    PERFORM b.slug FROM content.boards b
        WHERE b.slug=p_board FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Cleanup board is unavailable.' USING ERRCODE='P0117';
    END IF;
    v_cutoff := ((clock_timestamp() AT TIME ZONE 'UTC') - interval '2 years') AT TIME ZONE 'UTC';
    DELETE FROM post_secrets.robot9000_texts h
        WHERE h.board=p_board AND h.digest IN (
            SELECT old.digest FROM post_secrets.robot9000_texts old
            WHERE old.board=p_board AND old.seen_at<v_cutoff
            ORDER BY old.seen_at,old.digest LIMIT 1000
        );
    GET DIAGNOSTICS v_removed = ROW_COUNT;
    SELECT EXISTS(SELECT 1 FROM post_secrets.robot9000_texts h
        WHERE h.board=p_board AND h.seen_at<v_cutoff) INTO v_more;
    INSERT INTO content.board_cleanup_audit(account_id,board,action,cutoff,removed)
        VALUES(p_account,p_board,'robot9000-texts',v_cutoff,v_removed);
    RETURN QUERY SELECT v_removed,v_more,v_cutoff;
END $$;
REVOKE ALL ON FUNCTION content.cleanup_robot9000(text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.cleanup_robot9000(text,bigint) TO board_staff;
GRANT CREATE ON SCHEMA content TO board_robot9000_owner;
ALTER FUNCTION content.cleanup_robot9000(text,bigint) OWNER TO board_robot9000_owner;
REVOKE CREATE ON SCHEMA content FROM board_robot9000_owner;
