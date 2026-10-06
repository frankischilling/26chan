-- imgboard.php:5862-5866,5887-5900: named/meta janitors retain ordinary
-- admission, with ceil-half reply/image delays, followed by the staff 5s gate.
-- The parsed raw-name fact is bound before display-name normalization.
ALTER TABLE post_secrets.staff_post_intents
    ADD COLUMN raw_name_nonempty boolean,
    ADD COLUMN is_janitor boolean,
    ADD COLUMN meta_board boolean,
    ADD CONSTRAINT staff_post_timer_context_check CHECK (
        (raw_name_nonempty IS NULL AND is_janitor IS NULL AND meta_board IS NULL)
        OR (raw_name_nonempty IS NOT NULL AND is_janitor IS NOT NULL AND meta_board IS NOT NULL));
-- Existing 15-second intents have no trustworthy raw-name fact. Keep their
-- normal expiry, but reject them at consumption rather than guessing it.
GRANT UPDATE(raw_name_nonempty,is_janitor,meta_board)
    ON post_secrets.staff_post_intents TO board_staff_post_owner;
GRANT CREATE ON SCHEMA staff_identity,content TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;

-- Owner-only helper called in the issuer's same SQL statement, retaining its
-- account/session locks. Never lock the board on this independent auth pool:
-- the posting transaction already owns that lock. Consumption rechecks policy.
CREATE FUNCTION staff_identity.bind_post_timer_context(ticket bytea,v_raw_name_nonempty boolean)
RETURNS boolean LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_janitor boolean; v_meta boolean;
BEGIN
    IF v_raw_name_nonempty IS NULL THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT a.role='janitor',b.meta_board INTO v_janitor,v_meta
        FROM post_secrets.staff_post_intents i
        JOIN staff_identity.accounts a ON a.id=i.account_id
        JOIN content.boards b ON b.slug=i.board
        WHERE i.token_hash=ticket AND a.revoked_at IS NULL
        AND a.role IN ('janitor','moderator','manager','admin');
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
    UPDATE post_secrets.staff_post_intents SET raw_name_nonempty=v_raw_name_nonempty,
        is_janitor=v_janitor,meta_board=v_meta WHERE token_hash=ticket
        AND raw_name_nonempty IS NULL AND is_janitor IS NULL AND meta_board IS NULL;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
    RETURN v_janitor AND (v_raw_name_nonempty OR v_meta);
END $$;
REVOKE ALL ON FUNCTION staff_identity.bind_post_timer_context(bytea,boolean)
    FROM PUBLIC,board_public,board_staff,board_auth;

CREATE FUNCTION staff_identity.issue_limited_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,highlight boolean,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_authorized boolean,v_comment_limit integer,v_wordfilter_payload bytea,v_wordfilter_search text,v_raw_name_nonempty boolean
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    PERFORM staff_identity.issue_limited_post_authority(ticket,session_token,csrf,idle,highlight,post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time,v_authorized,v_comment_limit,v_wordfilter_payload,v_wordfilter_search);
    RETURN staff_identity.bind_post_timer_context(ticket,v_raw_name_nonempty);
END $$;
REVOKE ALL ON FUNCTION staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text)
    FROM PUBLIC,board_public,board_staff,board_auth;
REVOKE ALL ON FUNCTION staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,boolean)
    FROM PUBLIC,board_public,board_staff;
GRANT EXECUTE ON FUNCTION staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,boolean) TO board_auth;

CREATE FUNCTION staff_identity.issue_source_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,highlight boolean,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_authorized boolean,v_comment_limit integer,v_wordfilter_payload bytea,v_wordfilter_search text,
    v_options text,v_trip text,v_name_allowed boolean,v_raw_name_nonempty boolean
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    PERFORM staff_identity.issue_source_post_authority(ticket,session_token,csrf,idle,highlight,post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time,v_authorized,v_comment_limit,v_wordfilter_payload,v_wordfilter_search,v_options,v_trip,v_name_allowed);
    RETURN staff_identity.bind_post_timer_context(ticket,v_raw_name_nonempty);
END $$;
REVOKE ALL ON FUNCTION staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean)
    FROM PUBLIC,board_public,board_staff,board_auth;
REVOKE ALL ON FUNCTION staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,boolean)
    FROM PUBLIC,board_public,board_staff;
GRANT EXECUTE ON FUNCTION staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,boolean) TO board_auth;

CREATE FUNCTION staff_identity.issue_ordinary_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_authorized boolean,v_comment_limit integer,v_wordfilter_payload bytea,v_wordfilter_search text,
    v_options text,v_trip text,v_name_allowed boolean,v_context jsonb,v_raw_name_nonempty boolean
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    PERFORM staff_identity.issue_ordinary_post_authority(ticket,session_token,csrf,idle,post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time,v_authorized,v_comment_limit,v_wordfilter_payload,v_wordfilter_search,v_options,v_trip,v_name_allowed,v_context);
    RETURN staff_identity.bind_post_timer_context(ticket,v_raw_name_nonempty);
END $$;
REVOKE ALL ON FUNCTION staff_identity.issue_ordinary_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb)
    FROM PUBLIC,board_public,board_staff,board_auth;
REVOKE ALL ON FUNCTION staff_identity.issue_ordinary_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb,boolean)
    FROM PUBLIC,board_public,board_staff;
GRANT EXECUTE ON FUNCTION staff_identity.issue_ordinary_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb,boolean) TO board_auth;

-- Retire older auth entrypoints as well: they cannot bind timer context and
-- must not reserve intent capacity. Owner-internal calls remain available.
REVOKE ALL ON FUNCTION staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz),
    staff_identity.issue_wordfiltered_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,bytea,text)
    FROM PUBLIC,board_public,board_staff,board_auth;

-- Preserve the complete existing ordinary/badged/private proof consumer.
-- Only the new wrapper is callable by the runtime or insertion trigger.
ALTER FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)
    RENAME TO consume_staff_post_authority_core;
REVOKE ALL ON FUNCTION content.consume_staff_post_authority_core(bytea,bigint,text,bigint,text,text,text,timestamptz)
    FROM PUBLIC,board_public,board_staff,board_auth;

CREATE FUNCTION content.consume_staff_post_authority(ticket bytea,post_number bigint,v_board text,
    v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE intent post_secrets.staff_post_intents%ROWTYPE; locked_intent post_secrets.staff_post_intents%ROWTYPE;
    actual_role text; actual_meta boolean; raw_fact text;
BEGIN
    SELECT * INTO intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket;
    raw_fact:=current_setting('board.staff_raw_name_nonempty',true);
    IF NOT FOUND OR intent.raw_name_nonempty IS NULL OR intent.is_janitor IS NULL OR intent.meta_board IS NULL
        OR raw_fact IS NULL OR raw_fact NOT IN ('true','false')
        OR raw_fact IS DISTINCT FROM intent.raw_name_nonempty::text
        OR intent.post_id IS DISTINCT FROM post_number OR intent.board IS DISTINCT FROM v_board
        OR intent.thread_id IS DISTINCT FROM v_thread OR intent.name IS DISTINCT FROM v_name
        OR intent.subject IS DISTINCT FROM v_subject OR intent.comment IS DISTINCT FROM v_comment
        OR intent.posted_at IS DISTINCT FROM v_time THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT b.meta_board INTO actual_meta FROM content.boards b WHERE b.slug=v_board;
    IF NOT FOUND OR actual_meta IS DISTINCT FROM intent.meta_board THEN
        RAISE EXCEPTION 'Staff posting policy changed' USING ERRCODE='28000';
    END IF;
    -- Match revocation order: account/session SHARE, then single-use intent UPDATE.
    SELECT a.role INTO actual_role FROM staff_identity.accounts a
        JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id
        AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes' AND a.revoked_at IS NULL
        AND a.role IN ('janitor','moderator','manager','admin')
        AND staff_identity.has_board_access(a.id,v_board) FOR SHARE OF a,s;
    IF NOT FOUND OR (actual_role='janitor') IS DISTINCT FROM intent.is_janitor THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT * INTO locked_intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket FOR UPDATE;
    IF NOT FOUND OR locked_intent IS DISTINCT FROM intent OR intent.expires_at<=clock_timestamp()
        OR NOT EXISTS(SELECT 1 FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
            WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id
            AND s.expires_at>clock_timestamp()
            AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
            AND s.authenticated_at>clock_timestamp()-interval '10 minutes' AND a.revoked_at IS NULL
            AND a.role=actual_role AND staff_identity.has_board_access(a.id,v_board)) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    RETURN content.consume_staff_post_authority_core(ticket,post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time);
END $$;
REVOKE ALL ON FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)
    FROM PUBLIC,board_public,board_auth;
GRANT EXECUTE ON FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)
    TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,staff_identity FROM board_staff_post_owner;

-- Rebind this small invoker trigger to the new wrapper after the rename.
CREATE OR REPLACE FUNCTION content.apply_staff_capcode() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE ticket text;
BEGIN
    NEW.capcode:=NULL;
    NEW.staff_authorized_limits:=false;
    PERFORM set_config('board.staff_is_admin','false',true),set_config('board.staff_ordinary_post','false',true);
    IF current_user='board_staff' THEN
        PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
        IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
        ticket:=current_setting('board.staff_post_ticket',true);
        IF ticket IS NULL OR ticket !~ '^[0-9a-f]{64}$' THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
        NEW.capcode:=content.consume_staff_post_authority(decode(ticket,'hex'),NEW.id,NEW.board,
            NEW.thread_id,NEW.name,NEW.subject,NEW.comment,NEW.created_at);
        NEW.staff_authorized_limits:=current_setting('board.staff_authorized_limits',true)='true';
        IF current_setting('board.staff_ordinary_post',true) IS DISTINCT FROM 'true' THEN
            PERFORM set_config('board.poster_fingerprint','',true),set_config('board.poster_epoch','',true);
        END IF;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_staff_capcode() FROM PUBLIC;

-- The cooldown owner still has no account/session/intent access. Only the
-- staff application can request the janitor wrapper after verified issuance.
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
SET LOCAL ROLE board_posting_cooldown_owner;
CREATE FUNCTION content.check_posting_cooldown_core(
    p_actor bytea,p_board text,p_thread bigint,p_has_attachment boolean,p_request_at bigint,p_half_reply boolean
) RETURNS TABLE(kind text,remaining_seconds bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_reply integer; v_image integer; v_thread integer;
    v_delay integer; v_last bigint; v_now bigint; v_staff_only boolean;
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_thread IS NULL OR p_thread<0
        OR p_has_attachment IS NULL OR p_request_at IS NULL OR p_half_reply IS NULL
        OR p_request_at NOT BETWEEN 0 AND 9223372036854689407 THEN
        RAISE EXCEPTION 'Invalid posting context.' USING ERRCODE='23514';
    END IF;
    -- The caller already holds its actor/action gates. Reuse the authoritative
    -- board lock so operator changes and concurrent board mutations serialize.
    SELECT b.posting_reply_seconds,b.posting_image_seconds,b.posting_thread_seconds,b.staff_only
        INTO v_reply,v_image,v_thread,v_staff_only FROM content.boards b WHERE b.slug=p_board FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002'; END IF;
    IF v_staff_only AND (CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END) NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002';
    END IF;
    IF p_thread>0 THEN
        SELECT h.request_at INTO v_last FROM post_secrets.posting_history h
            WHERE h.actor_hash=p_actor AND h.board=p_board AND h.post_id<>h.thread_id
            ORDER BY h.post_id DESC LIMIT 1;
        v_delay:=CASE WHEN p_has_attachment THEN v_image ELSE v_reply END;
        IF p_half_reply THEN v_delay:=(v_delay+1)/2; END IF;
        IF v_last>p_request_at-v_delay THEN
            kind:=CASE WHEN p_has_attachment THEN 'image' ELSE 'reply' END;
            remaining_seconds:=v_last+v_delay-p_request_at;
            RETURN NEXT;
        END IF;
        RETURN;
    END IF;
    -- Sticky status does not discard the surviving OP's posting identity.
    SELECT max(h.request_at) INTO v_last FROM post_secrets.posting_history h
        WHERE h.actor_hash=p_actor AND h.board=p_board AND h.post_id=h.thread_id;
    IF v_last>p_request_at-v_thread THEN
        kind:='thread'; remaining_seconds:=v_last+v_thread-p_request_at;
        RETURN NEXT; RETURN;
    END IF;
    -- Cross-board uses the DB clock after contended locks, not request time.
    -- Its lower edge is inclusive, unlike the same-board strict comparison.
    v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
    SELECT max(a.request_at) INTO v_last FROM post_secrets.posting_thread_actions a
        WHERE a.actor_hash=p_actor AND a.board<>p_board AND a.request_at>=v_now-300;
    IF v_last IS NOT NULL THEN
        kind:='cross_board_thread'; remaining_seconds:=v_last+301-v_now;
        RETURN NEXT;
    END IF;
END $$;

REVOKE ALL ON FUNCTION content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)
    FROM PUBLIC,board_public,board_staff,board_auth;
CREATE OR REPLACE FUNCTION content.check_posting_cooldown(
    p_actor bytea,p_board text,p_thread bigint,p_has_attachment boolean,p_request_at bigint
) RETURNS TABLE(kind text,remaining_seconds bigint)
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    SELECT * FROM content.check_posting_cooldown_core(p_actor,p_board,p_thread,p_has_attachment,p_request_at,false)
$$;
CREATE FUNCTION content.check_janitor_posting_cooldown(
    p_actor bytea,p_board text,p_thread bigint,p_has_attachment boolean,p_request_at bigint
) RETURNS TABLE(kind text,remaining_seconds bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF (CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END) NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    RETURN QUERY SELECT * FROM content.check_posting_cooldown_core(
        p_actor,p_board,p_thread,p_has_attachment,p_request_at,true);
END $$;
REVOKE ALL ON FUNCTION content.check_janitor_posting_cooldown(bytea,text,bigint,boolean,bigint)
    FROM PUBLIC,board_public,board_auth;
GRANT EXECUTE ON FUNCTION content.check_janitor_posting_cooldown(bytea,text,bigint,boolean,bigint) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;
