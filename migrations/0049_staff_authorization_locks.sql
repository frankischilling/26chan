GRANT SELECT(id,account_id) ON staff_identity.credentials TO board_staff_post_owner;
GRANT UPDATE(last_activity_at) ON staff_identity.sessions TO board_staff_post_owner;

CREATE FUNCTION staff_identity.lock_session(session_token bytea,idle integer)
RETURNS TABLE(account_id bigint,role text,csrf_hash bytea,recent boolean,
    allow_boards text[],deny_boards text[],flags text[])
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE actor bigint;
BEGIN
    IF octet_length(session_token)<>32 OR idle NOT BETWEEN 60 AND 3600 THEN RETURN; END IF;
    SELECT s.account_id INTO actor FROM staff_identity.sessions s WHERE s.token_hash=session_token;
    IF NOT FOUND THEN RETURN; END IF;
    -- Keep operator revocation and scope changes ordered before the session lock.
    PERFORM a.id FROM staff_identity.accounts a WHERE a.id=actor FOR SHARE;
    IF NOT FOUND THEN RETURN; END IF;
    PERFORM s.token_hash FROM staff_identity.sessions s WHERE s.token_hash=session_token FOR UPDATE;
    IF NOT FOUND THEN RETURN; END IF;
    RETURN QUERY UPDATE staff_identity.sessions s SET last_activity_at=clock_timestamp()
      FROM staff_identity.accounts a,staff_identity.credentials c
      WHERE s.token_hash=session_token AND s.account_id=actor AND a.id=actor
        AND c.id=s.credential_id AND c.account_id=a.id
        AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>idle)
        AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin')
      RETURNING s.account_id,a.role,s.csrf_hash,
        s.authenticated_at>clock_timestamp()-interval '10 minutes',a.allow_boards,a.deny_boards,a.flags;
END $$;
REVOKE ALL ON FUNCTION staff_identity.lock_session(bytea,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.lock_session(bytea,integer) TO board_auth;
GRANT CREATE ON SCHEMA staff_identity TO board_staff_post_owner;
ALTER FUNCTION staff_identity.lock_session(bytea,integer) OWNER TO board_staff_post_owner;
REVOKE CREATE ON SCHEMA staff_identity FROM board_staff_post_owner;
