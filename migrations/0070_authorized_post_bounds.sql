-- Larger saved fields require a consumed, current moderator-or-higher proof.
-- Historical bodies and formats remain unchanged; existing proofs stay ordinary.
ALTER TABLE content.posts ADD COLUMN staff_authorized_limits boolean NOT NULL DEFAULT false;
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN authorized_limits boolean NOT NULL DEFAULT false;
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN comment_limit integer
    CHECK(comment_limit IS NULL OR comment_limit BETWEEN 1 AND 50000);
GRANT UPDATE(authorized_limits,comment_limit,comment) ON post_secrets.staff_post_intents TO board_staff_post_owner;

ALTER TABLE content.posts DROP CONSTRAINT posts_subject_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_subject_check CHECK (
    octet_length(subject)<=CASE WHEN staff_authorized_limits THEN 1020 ELSE 400 END);
ALTER TABLE content.posts DROP CONSTRAINT posts_wordfilter_payload_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_wordfilter_payload_check CHECK (
    wordfilter_payload IS NULL OR (
        octet_length(wordfilter_payload) BETWEEN 11 AND CASE WHEN staff_authorized_limits THEN 524288 ELSE 131072 END
        AND substring(wordfilter_payload FROM 1 FOR 4)=decode(CASE WHEN staff_authorized_limits THEN '57463032' ELSE '57463031' END,'hex')));
ALTER TABLE content.posts DROP CONSTRAINT posts_wordfilter_search_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_wordfilter_search_check CHECK (
    (wordfilter_payload IS NULL AND wordfilter_search IS NULL)
    OR (wordfilter_payload IS NOT NULL AND wordfilter_search IS NOT NULL
        AND octet_length(wordfilter_search)<=CASE WHEN staff_authorized_limits THEN 524288 ELSE 131072 END));
ALTER TABLE content.posts DROP CONSTRAINT posts_comment_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_check CHECK (
    (wordfilter_payload IS NULL AND char_length(comment) BETWEEN 0 AND CASE WHEN staff_authorized_limits THEN 200000 ELSE 16000 END
        AND octet_length(comment)<=CASE WHEN staff_authorized_limits THEN 200000 ELSE 64000 END)
    OR (wordfilter_payload IS NOT NULL AND octet_length(comment)<=CASE WHEN staff_authorized_limits THEN 2097152 ELSE 524288 END));

ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_name_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_name_check CHECK(octet_length(name) BETWEEN 1 AND 255);
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_subject_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_subject_check CHECK(octet_length(subject)<=1020);
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_comment_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_comment_check CHECK(
    octet_length(comment)<=CASE WHEN authorized_limits THEN 2097152 ELSE 524288 END);
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_wordfilter_payload_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_wordfilter_payload_check CHECK (
    wordfilter_payload IS NULL OR (
        octet_length(wordfilter_payload) BETWEEN 11 AND CASE WHEN authorized_limits THEN 524288 ELSE 131072 END
        AND substring(wordfilter_payload FROM 1 FOR 4)=decode(CASE WHEN authorized_limits THEN '57463032' ELSE '57463031' END,'hex')));
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_wordfilter_search_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_wordfilter_search_check CHECK(
    wordfilter_search IS NULL OR octet_length(wordfilter_search)<=CASE WHEN authorized_limits THEN 524288 ELSE 131072 END);

GRANT CREATE ON SCHEMA staff_identity,content TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;

CREATE FUNCTION staff_identity.issue_limited_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,highlight boolean,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_authorized boolean,v_comment_limit integer,v_wordfilter_payload bytea,v_wordfilter_search text
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE actual_role text; expected_limit integer;
BEGIN
    IF v_authorized IS NULL OR v_comment_limit IS NULL OR v_comment_limit NOT BETWEEN 1 AND 50000
        OR v_name IS NULL OR v_subject IS NULL OR v_comment IS NULL OR v_time IS NULL
        OR v_board IS NULL OR post_number IS NULL OR v_thread IS NULL
        OR octet_length(v_comment)>(CASE WHEN v_authorized THEN 2097152 ELSE 524288 END)
        OR (v_wordfilter_payload IS NULL)<>(v_wordfilter_search IS NULL)
        OR octet_length(v_wordfilter_search)>(CASE WHEN v_authorized THEN 524288 ELSE 131072 END) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF v_wordfilter_payload IS NOT NULL AND (
        octet_length(v_wordfilter_payload) NOT BETWEEN 11 AND (CASE WHEN v_authorized THEN 524288 ELSE 131072 END)
        OR substring(v_wordfilter_payload FROM 1 FOR 4)<>decode(CASE WHEN v_authorized THEN '57463032' ELSE '57463031' END,'hex')) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    -- The existing function locks account/session and checks scope, recent
    -- authentication, revocation, capacity and /j/. The final payload is set
    -- in this same statement, so no temporary proof is visible externally.
    PERFORM staff_identity.issue_post_authority(ticket,session_token,csrf,idle,highlight,
        post_number,v_board,v_thread,v_name,v_subject,'',v_time);
    SELECT a.role INTO actual_role FROM staff_identity.accounts a
      JOIN post_secrets.staff_post_intents i ON i.account_id=a.id WHERE i.token_hash=ticket;
    IF NOT FOUND OR (v_authorized AND actual_role NOT IN ('moderator','manager','admin')) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT CASE WHEN v_authorized THEN b.max_authorized_comment_chars ELSE b.max_comment_chars END
      INTO expected_limit FROM content.boards b WHERE b.slug=v_board;
    IF NOT FOUND OR v_comment_limit IS DISTINCT FROM expected_limit THEN
        RAISE EXCEPTION 'Staff posting policy changed' USING ERRCODE='28000';
    END IF;
    UPDATE post_secrets.staff_post_intents SET authorized_limits=v_authorized,comment_limit=v_comment_limit,
        comment=v_comment,wordfilter_payload=v_wordfilter_payload,wordfilter_search=v_wordfilter_search WHERE token_hash=ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
END $$;
REVOKE ALL ON FUNCTION staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text) TO board_auth;
CREATE OR REPLACE FUNCTION content.consume_staff_post_authority(ticket bytea,post_number bigint,v_board text,
    v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE intent post_secrets.staff_post_intents%ROWTYPE; locked_intent post_secrets.staff_post_intents%ROWTYPE; label text; v_role text; expected_limit integer;
BEGIN
    -- Read the immutable payload before taking authority locks. Lock the
    -- account and session before the intent, matching operator revocation;
    -- its session deletion cascades to this same private table.
    SELECT * INTO intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket;
    IF NOT FOUND OR intent.post_id IS DISTINCT FROM post_number OR intent.board IS DISTINCT FROM v_board
        OR intent.thread_id IS DISTINCT FROM v_thread OR intent.name IS DISTINCT FROM v_name
        OR intent.subject IS DISTINCT FROM v_subject OR intent.comment IS DISTINCT FROM v_comment
        OR intent.posted_at IS DISTINCT FROM v_time
        OR intent.wordfilter_payload IS DISTINCT FROM decode(nullif(current_setting('board.wordfilter_payload',true),''),'hex')
        OR intent.wordfilter_search IS DISTINCT FROM (CASE WHEN intent.wordfilter_payload IS NOT NULL THEN current_setting('board.wordfilter_search',true) ELSE NULL END) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT CASE WHEN intent.authorized_limits THEN b.max_authorized_comment_chars ELSE b.max_comment_chars END
      INTO expected_limit FROM content.boards b WHERE b.slug=v_board;
    IF NOT FOUND OR (intent.comment_limit IS NOT NULL AND intent.comment_limit IS DISTINCT FROM expected_limit) THEN
        RAISE EXCEPTION 'Staff posting policy changed' USING ERRCODE='28000';
    END IF;
    SELECT CASE WHEN v_board='j' THEN (CASE a.role WHEN 'moderator' THEN 'mod' ELSE a.role END) ELSE coalesce(a.public_capcode,CASE a.role WHEN 'admin' THEN 'admin' WHEN 'manager' THEN 'manager' ELSE 'mod' END) END,a.role
      INTO label,v_role FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
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
       OR (intent.authorized_limits AND v_role NOT IN ('moderator','manager','admin'))
       OR intent.expires_at<=clock_timestamp() OR NOT EXISTS (
        SELECT 1 FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id AND s.expires_at>clock_timestamp()
          AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
          AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
          AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin') AND (a.role<>'janitor' OR v_board='j') AND staff_identity.has_board_access(a.id,v_board))
       OR label IS DISTINCT FROM (CASE intent.capcode WHEN 'admin_highlight' THEN 'admin' ELSE intent.capcode END) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    PERFORM set_config('board.staff_authorized_limits',CASE WHEN intent.authorized_limits THEN 'true' ELSE 'false' END,true);
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
REVOKE CREATE ON SCHEMA staff_identity,content FROM board_staff_post_owner;

CREATE OR REPLACE FUNCTION content.apply_staff_capcode() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE ticket text;
BEGIN
    NEW.capcode:=NULL;
    NEW.staff_authorized_limits:=false;
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
        PERFORM set_config('board.poster_fingerprint','',true),set_config('board.poster_epoch','',true);
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION content.stamp_wordfilter_payload() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean; payload text; maximum integer;
BEGIN
    SELECT b.word_filter_enabled INTO enabled FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    NEW.wordfilter_payload:=NULL;
    NEW.wordfilter_search:=NULL;
    IF enabled THEN
        payload:=current_setting('board.wordfilter_payload',true);
        maximum:=CASE WHEN NEW.staff_authorized_limits THEN 1048576 ELSE 262144 END;
        IF payload IS NULL OR length(payload) NOT BETWEEN 22 AND maximum OR payload !~ '^[0-9a-f]+$'
            OR length(payload)%2<>0 THEN
            RAISE EXCEPTION 'Wordfilter result is unavailable.' USING ERRCODE='23514';
        END IF;
        NEW.wordfilter_payload:=decode(payload,'hex');
        NEW.wordfilter_search:=current_setting('board.wordfilter_search',true);
    ELSE
        IF current_user='board_staff' AND coalesce(current_setting('board.wordfilter_payload',true),'')<>'' THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
        PERFORM set_config('board.wordfilter_payload','',true);
        PERFORM set_config('board.wordfilter_search','',true);
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.stamp_wordfilter_payload() FROM PUBLIC;
