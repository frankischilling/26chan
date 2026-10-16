-- New generated-aware rows retain the complete typed source composition.
-- Existing payloads, profiles, posts and moderation snapshots are unchanged.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before generated formatting migration'; END IF;
END $$;
RESET ROLE;

ALTER TABLE content.posts DROP CONSTRAINT posts_wordfilter_payload_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_wordfilter_payload_check CHECK (
    wordfilter_payload IS NULL OR (
        octet_length(wordfilter_payload) BETWEEN 11 AND CASE WHEN staff_authorized_limits THEN 524288 ELSE 131072 END
        AND substring(wordfilter_payload FROM 1 FOR 4) IN (
            decode(CASE WHEN staff_authorized_limits THEN '57463032' ELSE '57463031' END,'hex'),
            decode(CASE WHEN staff_authorized_limits THEN '57463034' ELSE '57463033' END,'hex'))));

ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_wordfilter_payload_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_wordfilter_payload_check CHECK (
    wordfilter_payload IS NULL OR (
        octet_length(wordfilter_payload) BETWEEN 11 AND CASE WHEN authorized_limits THEN 524288 ELSE 131072 END
        AND substring(wordfilter_payload FROM 1 FOR 4) IN (
            decode(CASE WHEN authorized_limits THEN '57463032' ELSE '57463031' END,'hex'),
            decode(CASE WHEN authorized_limits THEN '57463034' ELSE '57463033' END,'hex'))));

ALTER TABLE content.posts DROP CONSTRAINT posts_comment_format_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_format_check CHECK (
    comment_format=0 OR comment_format BETWEEN 8 AND 15 OR comment_format BETWEEN 24 AND 31
    OR comment_format BETWEEN 40 AND 47 OR comment_format BETWEEN 56 AND 63
    OR comment_format BETWEEN 104 AND 111 OR comment_format BETWEEN 120 AND 127
    OR comment_format BETWEEN 232 AND 239 OR comment_format BETWEEN 248 AND 255
    OR comment_format BETWEEN 488 AND 495 OR comment_format BETWEEN 504 AND 511);

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_snapshot_comment_format_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_snapshot_comment_format_check CHECK (
    snapshot_comment_format=0 OR snapshot_comment_format BETWEEN 8 AND 15 OR snapshot_comment_format BETWEEN 24 AND 31
    OR snapshot_comment_format BETWEEN 40 AND 47 OR snapshot_comment_format BETWEEN 56 AND 63
    OR snapshot_comment_format BETWEEN 104 AND 111 OR snapshot_comment_format BETWEEN 120 AND 127
    OR snapshot_comment_format BETWEEN 232 AND 239 OR snapshot_comment_format BETWEEN 248 AND 255
    OR snapshot_comment_format BETWEEN 488 AND 495 OR snapshot_comment_format BETWEEN 504 AND 511);

ALTER TABLE content.posts ADD CONSTRAINT posts_generated_payload_check CHECK (
    (coalesce(substring(wordfilter_payload FROM 1 FOR 4) IN (decode('57463033','hex'),decode('57463034','hex')),false))
    = (comment_format BETWEEN 488 AND 495 OR comment_format BETWEEN 504 AND 511)
    AND (NOT (comment_format BETWEEN 488 AND 495 OR comment_format BETWEEN 504 AND 511)
        OR (dice_result IS NOT NULL OR fortune_text IS NOT NULL)));

GRANT CREATE ON SCHEMA staff_identity TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;
CREATE OR REPLACE FUNCTION staff_identity.issue_limited_post_authority(
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
        OR substring(v_wordfilter_payload FROM 1 FOR 4) NOT IN (decode(CASE WHEN v_authorized THEN '57463032' ELSE '57463031' END,'hex'),decode(CASE WHEN v_authorized THEN '57463034' ELSE '57463033' END,'hex'))) THEN
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

CREATE OR REPLACE FUNCTION staff_identity.issue_ordinary_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_authorized boolean,v_comment_limit integer,v_wordfilter_payload bytea,v_wordfilter_search text,
    v_options text,v_trip text,v_name_allowed boolean,v_context jsonb
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE actor bigint; actual_role text; flags text[]; allow_boards text[]; deny_boards text[];
    expected_limit integer; policy jsonb;
BEGIN
    IF ticket IS NULL OR octet_length(ticket)<>32 OR session_token IS NULL OR octet_length(session_token)<>32
        OR csrf IS NULL OR octet_length(csrf)<>32 OR idle IS NULL OR idle NOT BETWEEN 60 AND 3600
        OR v_board IS NULL OR v_board='j' OR post_number IS NULL OR post_number<=0
        OR v_thread IS NULL OR v_thread NOT BETWEEN 1 AND post_number
        OR v_authorized IS NULL OR v_comment_limit IS NULL OR v_comment_limit NOT BETWEEN 1 AND 50000
        OR v_name IS NULL OR v_subject IS NULL OR v_comment IS NULL OR v_time IS NULL
        OR octet_length(v_subject)>(CASE WHEN v_authorized THEN 1020 ELSE 400 END)
        OR octet_length(v_comment)>(CASE WHEN v_authorized THEN 2097152 ELSE 524288 END)
        OR (v_wordfilter_payload IS NULL)<>(v_wordfilter_search IS NULL)
        OR octet_length(v_wordfilter_search)>(CASE WHEN v_authorized THEN 524288 ELSE 131072 END) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF v_wordfilter_payload IS NOT NULL AND (
        octet_length(v_wordfilter_payload) NOT BETWEEN 11 AND (CASE WHEN v_authorized THEN 524288 ELSE 131072 END)
        OR substring(v_wordfilter_payload FROM 1 FOR 4) NOT IN (decode(CASE WHEN v_authorized THEN '57463032' ELSE '57463031' END,'hex'),decode(CASE WHEN v_authorized THEN '57463034' ELSE '57463033' END,'hex'))) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT a.id,a.role,a.flags,a.allow_boards,a.deny_boards INTO actor,actual_role,flags,allow_boards,deny_boards
        FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE s.token_hash=session_token AND s.csrf_hash=csrf AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>idle)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
        AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin')
        AND staff_identity.has_board_access(a.id,v_board) FOR UPDATE OF a FOR SHARE OF s;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
    -- Recheck time bounds after any authority lock wait.
    IF NOT EXISTS(SELECT 1 FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE a.id=actor AND s.token_hash=session_token AND s.csrf_hash=csrf
        AND s.expires_at>clock_timestamp() AND s.last_activity_at>clock_timestamp()-make_interval(secs=>idle)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes' AND a.revoked_at IS NULL
        AND a.role=actual_role AND staff_identity.has_board_access(a.id,v_board)) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT CASE WHEN v_authorized THEN max_authorized_comment_chars ELSE max_comment_chars END
        INTO expected_limit FROM content.boards WHERE slug=v_board AND NOT staff_only;
    policy:=content.staff_ordinary_policy(v_board);
    IF expected_limit IS DISTINCT FROM v_comment_limit OR policy IS NULL THEN
        RAISE EXCEPTION 'Staff posting policy changed' USING ERRCODE='28000';
    END IF;
    PERFORM staff_identity.check_ordinary_post_identity(actual_role,flags,allow_boards,deny_boards,v_options,
        v_name,v_trip,v_name_allowed,v_subject,policy,v_context,v_authorized);
    PERFORM content.check_staff_op_context(v_board,v_thread,post_number,v_context);
    DELETE FROM post_secrets.staff_post_intents WHERE token_hash IN (
        SELECT token_hash FROM post_secrets.staff_post_intents WHERE expires_at<=clock_timestamp()
        ORDER BY expires_at LIMIT 4096);
    IF (SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=actor)>=32 THEN
        RAISE EXCEPTION 'Staff posting capacity reached' USING ERRCODE='54000';
    END IF;
    INSERT INTO post_secrets.staff_post_intents(token_hash,session_hash,account_id,capcode,post_id,board,
        thread_id,name,subject,comment,posted_at,idle_seconds,authorized_limits,comment_limit,
        wordfilter_payload,wordfilter_search,source_options,prepared_trip,source_name_allowed,
        ordinary,ordinary_context,ordinary_policy)
        VALUES(ticket,session_token,actor,'none',post_number,v_board,v_thread,v_name,v_subject,v_comment,
            v_time,idle,v_authorized,v_comment_limit,v_wordfilter_payload,v_wordfilter_search,v_options,
            v_trip,v_name_allowed,true,v_context,policy);
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA staff_identity FROM board_staff_post_owner;

CREATE OR REPLACE FUNCTION content.stamp_wordfilter_payload() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean; payload text; maximum integer; generated boolean;
BEGIN
    SELECT b.word_filter_enabled INTO enabled FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    NEW.wordfilter_payload:=NULL;
    NEW.wordfilter_search:=NULL;
    payload:=current_setting('board.wordfilter_payload',true);
    generated:=coalesce(left(payload,8) IN ('57463033','57463034'),false);
    IF enabled OR generated THEN
        maximum:=CASE WHEN NEW.staff_authorized_limits THEN 1048576 ELSE 262144 END;
        IF payload IS NULL OR length(payload) NOT BETWEEN 22 AND maximum OR payload !~ '^[0-9a-f]+$'
            OR length(payload)%2<>0 OR (generated AND NEW.dice_result IS NULL AND NEW.fortune_text IS NULL) THEN
            RAISE EXCEPTION 'Wordfilter result is unavailable.' USING ERRCODE='23514';
        END IF;
        NEW.wordfilter_payload:=decode(payload,'hex');
        NEW.wordfilter_search:=current_setting('board.wordfilter_search',true);
    ELSE
        IF current_user='board_staff' AND coalesce(payload,'')<>'' THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
        PERFORM set_config('board.wordfilter_payload','',true);
        PERFORM set_config('board.wordfilter_search','',true);
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.stamp_wordfilter_payload() FROM PUBLIC;

CREATE OR REPLACE FUNCTION content.stamp_comment_format() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_format smallint;
BEGIN
    SELECT (232 + b.comment_spoiler_cleanup::integer
        + 2 * b.comment_code_spacing::integer
        + 4 * b.comment_sjis_spacing::integer
        + 16 * (b.op_markup AND (NEW.id=NEW.thread_id
            OR COALESCE(current_setting('board.source_op_reply', true)='true', false)))::integer)::smallint
        INTO v_format FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE = '23503';
    END IF;
    IF substring(NEW.wordfilter_payload FROM 1 FOR 4) IN (decode('57463033','hex'),decode('57463034','hex')) THEN
        v_format := v_format + 256;
    END IF;
    NEW.comment_format := v_format;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.stamp_comment_format() FROM PUBLIC;

SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Generated formatting migration did not preserve transfer authority'; END IF;
END $$;
RESET ROLE;
