-- The original /j/ board is never a public board, even if an operator changes
-- unrelated display settings. Authorship stays in the identity schema.
ALTER TABLE content.boards ADD CONSTRAINT private_janitor_board
    CHECK (slug<>'j' OR staff_only);
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_capcode_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_capcode_check
    CHECK (capcode IN ('mod','admin','admin_highlight','manager','developer','founder')
        OR (board='j' AND capcode='janitor'));

CREATE TABLE staff_identity.discussion_posts (
    post_id bigint PRIMARY KEY REFERENCES content.posts(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    account_id bigint NOT NULL REFERENCES staff_identity.accounts(id) ON DELETE CASCADE
);
CREATE INDEX discussion_posts_account ON staff_identity.discussion_posts(account_id);
REVOKE ALL ON staff_identity.discussion_posts FROM PUBLIC;
GRANT SELECT ON staff_identity.discussion_posts TO board_auth;
GRANT INSERT(post_id,account_id) ON staff_identity.discussion_posts TO board_staff_post_owner;

CREATE OR REPLACE FUNCTION staff_identity.has_board_access(actor bigint,v_board text) RETURNS boolean
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
    SELECT coalesce((SELECT ('all'=ANY(a.allow_boards) OR v_board=ANY(a.allow_boards)
        OR (v_board='j' AND a.role IN ('janitor','moderator','manager','admin')))
        AND NOT (CASE WHEN v_board='' THEN 'noboard' ELSE v_board END)=ANY(a.deny_boards)
        FROM staff_identity.accounts a WHERE a.id=actor AND a.revoked_at IS NULL),false)
$$;

-- These functions retain the existing non-login capability owner. The content
-- role cannot inspect the author mapping, and the auth role cannot insert posts.
GRANT CREATE ON SCHEMA content,staff_identity TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;

CREATE OR REPLACE FUNCTION staff_identity.issue_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,highlight boolean,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE actor bigint; label text;
BEGIN
    IF octet_length(ticket)<>32 OR octet_length(session_token)<>32 OR octet_length(csrf)<>32
       OR idle NOT BETWEEN 60 AND 3600 OR highlight IS NULL
       OR (v_board='j' AND (v_name IS DISTINCT FROM 'Anonymous' OR highlight)) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT a.id,CASE WHEN v_board='j' THEN (CASE a.role WHEN 'moderator' THEN 'mod' ELSE a.role END) ELSE coalesce(a.public_capcode,CASE a.role WHEN 'admin' THEN 'admin' WHEN 'manager' THEN 'manager' ELSE 'mod' END) END
      INTO actor,label FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
      WHERE s.token_hash=session_token AND s.csrf_hash=csrf AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>idle)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
        AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin') AND (a.role<>'janitor' OR v_board='j') AND staff_identity.has_board_access(a.id,v_board) FOR UPDATE OF a FOR SHARE OF s;
    IF NOT FOUND OR (highlight AND label<>'admin') OR NOT EXISTS (
        SELECT 1 FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE s.token_hash=session_token AND s.csrf_hash=csrf AND s.expires_at>clock_timestamp()
          AND s.last_activity_at>clock_timestamp()-make_interval(secs=>idle)
          AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
          AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin') AND (a.role<>'janitor' OR v_board='j') AND staff_identity.has_board_access(a.id,v_board)) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF highlight THEN label:='admin_highlight'; END IF;
    DELETE FROM post_secrets.staff_post_intents WHERE token_hash IN
        (SELECT token_hash FROM post_secrets.staff_post_intents WHERE expires_at<=clock_timestamp()
         ORDER BY expires_at LIMIT 4096);
    IF (SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=actor)>=32 THEN
        RAISE EXCEPTION 'Staff posting capacity reached' USING ERRCODE='54000';
    END IF;
    INSERT INTO post_secrets.staff_post_intents(token_hash,session_hash,account_id,capcode,
        post_id,board,thread_id,name,subject,comment,posted_at,idle_seconds)
      VALUES(ticket,session_token,actor,label,post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time,idle);
END $$;

CREATE OR REPLACE FUNCTION content.consume_staff_post_authority(ticket bytea,post_number bigint,v_board text,
    v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE intent post_secrets.staff_post_intents%ROWTYPE; locked_intent post_secrets.staff_post_intents%ROWTYPE; label text;
BEGIN
    -- Read the immutable payload before taking authority locks. Lock the
    -- account and session before the intent, matching operator revocation;
    -- its session deletion cascades to this same private table.
    SELECT * INTO intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket;
    IF NOT FOUND OR intent.post_id IS DISTINCT FROM post_number OR intent.board IS DISTINCT FROM v_board
        OR intent.thread_id IS DISTINCT FROM v_thread OR intent.name IS DISTINCT FROM v_name
        OR intent.subject IS DISTINCT FROM v_subject OR intent.comment IS DISTINCT FROM v_comment
        OR intent.posted_at IS DISTINCT FROM v_time THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT CASE WHEN v_board='j' THEN (CASE a.role WHEN 'moderator' THEN 'mod' ELSE a.role END) ELSE coalesce(a.public_capcode,CASE a.role WHEN 'admin' THEN 'admin' WHEN 'manager' THEN 'manager' ELSE 'mod' END) END
      INTO label FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
      WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id
        AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
        AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin') AND (a.role<>'janitor' OR v_board='j') AND staff_identity.has_board_access(a.id,v_board) FOR SHARE OF a,s;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT * INTO locked_intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket FOR UPDATE;
    IF NOT FOUND OR locked_intent IS DISTINCT FROM intent
       OR intent.expires_at<=clock_timestamp() OR NOT EXISTS (
        SELECT 1 FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id AND s.expires_at>clock_timestamp()
          AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
          AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
          AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin') AND (a.role<>'janitor' OR v_board='j') AND staff_identity.has_board_access(a.id,v_board))
       OR label IS DISTINCT FROM (CASE intent.capcode WHEN 'admin_highlight' THEN 'admin' ELSE intent.capcode END) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    DELETE FROM post_secrets.staff_post_intents WHERE token_hash=ticket;
    INSERT INTO content.moderation_audit(account_id,board,target_id,action)
      VALUES(intent.account_id,v_board,post_number,'staff-post');
    IF v_board='j' THEN
        INSERT INTO staff_identity.discussion_posts(post_id,account_id) VALUES(post_number,intent.account_id);
        RETURN NULL;
    END IF;
    RETURN intent.capcode;
END $$;


RESET ROLE;
REVOKE CREATE ON SCHEMA content,staff_identity FROM board_staff_post_owner;
