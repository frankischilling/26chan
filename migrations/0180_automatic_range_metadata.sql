-- Retain only derived mobile comparison metadata from accepted posts.
-- Existing posts keep NULL evidence; this migration installs no range events.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before automatic metadata'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before automatic metadata'; END IF;
END $$;
RESET ROLE;
ALTER TABLE post_secrets.posting_history
 ADD COLUMN automatic_browser text,
 ADD COLUMN automatic_prefix bytea,
 ADD CONSTRAINT posting_history_automatic_metadata CHECK (
  (automatic_browser IS NULL AND automatic_prefix IS NULL) OR
  (automatic_browser IS NOT NULL AND automatic_prefix IS NOT NULL
   AND automatic_browser ~ '^1[0-9a-f]{8}$' AND octet_length(automatic_prefix)=32));
GRANT SELECT(automatic_browser,automatic_prefix),INSERT(automatic_browser,automatic_prefix)
 ON post_secrets.posting_history TO board_thread_transfer_owner;
GRANT SELECT(automatic_browser,automatic_prefix) ON post_secrets.posting_history TO board_restriction_owner;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
SET LOCAL ROLE board_posting_cooldown_owner;
CREATE OR REPLACE FUNCTION content.record_inserted_posting_history()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_actor bytea; v_encoded text; v_invoker text;
    v_request bigint; v_now bigint; v_staff_only boolean;
    v_context text; v_meta jsonb; v_browser text; v_prefix bytea;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    v_encoded:=current_setting('board.posting_actor',true);
    -- Historical imports have no recoverable identity. An operator may supply
    -- an explicit actor, using exactly the same validation and capacity path.
    -- The attachment definer retains its original board_public invoker here.
    IF v_invoker='board_migrator' AND coalesce(v_encoded,'')='' THEN RETURN NEW; END IF;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Posting history registration is unavailable.' USING ERRCODE='23514';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Posting history requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF v_encoded IS NULL OR octet_length(v_encoded)<>64 OR v_encoded !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Invalid posting actor.' USING ERRCODE='23514';
    END IF;
    v_actor:=decode(v_encoded,'hex');
    -- The normal writer already owns these gates before any board/thread,
    -- admission, session or media lock. Late reentry never waits: a direct SQL
    -- writer that skipped that order fails closed instead of creating a cycle.
    PERFORM g.stripe FROM post_secrets.posting_actor_gates g
        WHERE g.stripe=(get_byte(v_actor,0)*256+get_byte(v_actor,1))%4096 FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Posting capacity is unavailable.' USING ERRCODE='P0087';
    END IF;
    IF NEW.id=NEW.thread_id THEN
        PERFORM c.singleton FROM post_secrets.posting_action_capacity c
            WHERE c.singleton FOR UPDATE NOWAIT;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'Posting capacity is unavailable.' USING ERRCODE='P0087';
        END IF;
    END IF;
    SELECT b.staff_only INTO v_staff_only FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE;
    IF NOT FOUND OR (v_staff_only AND v_invoker NOT IN ('board_staff','board_migrator')) THEN
        RAISE EXCEPTION 'Posting history registration is unavailable.' USING ERRCODE='23514';
    END IF;
    IF NEW.deleted OR NOT EXISTS(SELECT 1 FROM content.threads t
        WHERE t.id=NEW.thread_id AND t.board=NEW.board AND NOT t.deleted AND t.archived_at IS NULL) THEN
        RAISE EXCEPTION 'Posting history registration is unavailable.' USING ERRCODE='23514';
    END IF;
    v_request:=floor(extract(epoch FROM NEW.created_at))::bigint;
    -- A fixed 100,000-row per-board budget fails closed, never evicts live
    -- identity. There is intentionally no TTL: a later policy increase or a
    -- reversed request-time ordering must still see surviving source history.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.posting_history h
        WHERE h.board=NEW.board LIMIT 100000) bounded)>=100000 THEN
        RAISE EXCEPTION 'Posting history capacity is exhausted.' USING ERRCODE='P0087';
    END IF;
    -- The only runtime writer derives all three keys from NEW; the
    -- composite foreign keys additionally enforce both board associations.
    -- Optional derived comparison metadata is consumed exactly once. This
    -- shares the existing application-role trust boundary of posting_actor;
    -- it is not a credential or authority claim from a public form.
    v_context:=current_setting('board.automatic_range_metadata',true);
    PERFORM set_config('board.automatic_range_metadata','',true);
    IF coalesce(v_context,'')<>'' THEN
        IF octet_length(v_context)>512 THEN
            RAISE EXCEPTION 'Invalid automatic range metadata.' USING ERRCODE='P0180';
        END IF;
        BEGIN v_meta:=v_context::jsonb;
        EXCEPTION WHEN invalid_text_representation THEN
            RAISE EXCEPTION 'Invalid automatic range metadata.' USING ERRCODE='P0180';
        END;
        IF jsonb_typeof(v_meta) IS DISTINCT FROM 'object' THEN
            RAISE EXCEPTION 'Invalid automatic range metadata.' USING ERRCODE='P0180';
        END IF;
        IF (SELECT count(*) FROM jsonb_object_keys(v_meta))<>5
            OR v_meta->>'board' IS DISTINCT FROM NEW.board
            OR v_meta->>'parent' IS DISTINCT FROM (CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END)::text
            OR v_meta->>'actor' IS DISTINCT FROM encode(v_actor,'hex')
            OR coalesce(v_meta->>'browser','') !~ '^1[0-9a-f]{8}$'
            OR coalesce(v_meta->>'prefix','') !~ '^[0-9a-f]{64}$' THEN
            RAISE EXCEPTION 'Invalid automatic range metadata.' USING ERRCODE='P0180';
        END IF;
        v_browser:=v_meta->>'browser'; v_prefix:=decode(v_meta->>'prefix','hex');
    END IF;
    INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at,automatic_browser,automatic_prefix)
        VALUES(NEW.id,NEW.board,NEW.thread_id,v_actor,v_request,v_browser,v_prefix);
    IF NEW.id=NEW.thread_id THEN
        IF NOT EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions a
            WHERE a.actor_hash=v_actor AND a.board=NEW.board) THEN
            v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
            DELETE FROM post_secrets.posting_thread_actions a WHERE (a.actor_hash,a.board) IN (
                SELECT expired.actor_hash,expired.board FROM post_secrets.posting_thread_actions expired
                WHERE expired.request_at<v_now-300 ORDER BY expired.request_at
                LIMIT 64 FOR UPDATE SKIP LOCKED
            );
            IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.posting_thread_actions
                LIMIT 100000) bounded)>=100000 THEN
                RAISE EXCEPTION 'Posting action capacity is exhausted.' USING ERRCODE='P0087';
            END IF;
        END IF;
        INSERT INTO post_secrets.posting_thread_actions AS a(actor_hash,board,request_at)
            VALUES(v_actor,NEW.board,v_request)
            ON CONFLICT(actor_hash,board) DO UPDATE SET request_at=greatest(a.request_at,EXCLUDED.request_at);
    END IF;
    RETURN NEW;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;
GRANT CREATE ON SCHEMA content,post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
CREATE OR REPLACE FUNCTION content.consume_text_thread_transfer(p_ticket bytea)
RETURNS TABLE(request_id text,source_board text,source_thread bigint,destination_board text,
    destination_thread bigint,post_count bigint,audit_id bigint,source_post_id bigint,destination_post_id bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE i post_secrets.thread_transfer_intents%ROWTYPE; v_count integer; v_audit bigint; v_now timestamptz;
    v_inserted integer; v_anonymous_at bigint;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_staff' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM post_secrets.transfer_gate();
    SELECT * INTO i FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    IF post_secrets.transfer_authority(i.session_hash,i.csrf_hash,i.idle_seconds,
        i.source_board,i.destination_board,true,true,i.credential_id)<>i.account_id THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM ticket_hash FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket FOR UPDATE NOWAIT;
    IF NOT i.sealed THEN RAISE EXCEPTION 'Transfer is not sealed.' USING ERRCODE='P131I'; END IF;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    v_count:=post_secrets.transfer_lock_source(i.source_board,i.source_thread,i.destination_board);
    IF v_count<>i.post_count OR post_secrets.transfer_source_hash(i.source_board,i.source_thread,i.destination_board)
        IS DISTINCT FROM i.source_fingerprint OR EXISTS(
            SELECT 1 FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket
            AND post_secrets.transfer_post_hash(m.source_post_id) IS DISTINCT FROM m.source_hash) THEN
        RAISE EXCEPTION 'Transfer source changed.' USING ERRCODE='P131C'; END IF;
    IF (SELECT count(*) FROM post_secrets.thread_transfer_map WHERE ticket_hash=p_ticket)<>v_count
        OR (SELECT count(*) FROM post_secrets.thread_transfer_sealed_rows WHERE ticket_hash=p_ticket)<>v_count
        OR EXISTS(SELECT 1 FROM post_secrets.thread_transfer_map m JOIN content.posts p ON p.id=m.destination_post_id WHERE m.ticket_hash=p_ticket)
        OR EXISTS(SELECT 1 FROM content.threads t WHERE t.id=i.destination_thread) THEN
        RAISE EXCEPTION 'Transfer destination changed.' USING ERRCODE='P131C'; END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM content.threads t WHERE t.board=i.destination_board AND NOT t.deleted
        AND t.archived_at IS NULL AND NOT t.sticky AND NOT t.undead LIMIT 1001) occupied)>=
        (SELECT b.thread_limit FROM content.boards b WHERE b.slug=i.destination_board)
        OR (SELECT count(*) FROM (SELECT 1 FROM post_secrets.posting_history h WHERE h.board=i.destination_board LIMIT 100001) occupied)
          +(SELECT count(*) FROM post_secrets.posting_history h JOIN post_secrets.thread_transfer_map m
            ON m.source_post_id=h.post_id WHERE m.ticket_hash=p_ticket)>100000 THEN
        RAISE EXCEPTION 'Transfer destination capacity is exhausted.' USING ERRCODE='P131L'; END IF;
    -- Session before membership; NOWAIT avoids reverse-order expiry cleanup.
    PERFORM s.token_hash FROM post_secrets.anonymous_sessions s WHERE EXISTS(
        SELECT 1 FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.source_post_id=a.post_id
        WHERE m.ticket_hash=p_ticket AND a.token_hash=s.token_hash) ORDER BY s.token_hash FOR UPDATE NOWAIT;
    PERFORM d.post_id FROM post_secrets.deletion d JOIN post_secrets.thread_transfer_map m ON m.source_post_id=d.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY d.post_id FOR SHARE OF d NOWAIT;
    PERFORM o.thread_id FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread FOR SHARE NOWAIT;
    PERFORM r.post_id FROM post_secrets.op_replies r JOIN post_secrets.thread_transfer_map m ON m.source_post_id=r.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY r.post_id FOR SHARE OF r NOWAIT;
    PERFORM h.post_id FROM post_secrets.posting_history h JOIN post_secrets.thread_transfer_map m ON m.source_post_id=h.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY h.post_id FOR SHARE OF h NOWAIT;
    PERFORM a.post_id FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.source_post_id=a.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY a.post_id FOR SHARE OF a NOWAIT;
    PERFORM o.post_id FROM verification_private.post_ownership o JOIN post_secrets.thread_transfer_map m ON m.source_post_id=o.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY o.post_id FOR SHARE OF o NOWAIT;
    INSERT INTO post_secrets.thread_transfer_active
        SELECT pg_current_xact_id(),m.destination_post_id,i.destination_board,i.destination_thread,p_ticket
        FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket;
    v_now:=clock_timestamp();
    INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,http_modified_at,reply_count,
        sticky,sticky_rank,closed,permasage,permaage,undead,archived_at,archive_expires_at,deleted,content_erased)
    SELECT i.destination_thread,i.destination_board,p.created_at,v_now,p.created_at,v_now,v_count-1,
        false,0,false,false,false,false,NULL,NULL,false,false
    FROM content.posts p WHERE p.id=i.source_thread AND p.board=i.source_board;
    INSERT INTO content.posts(id,board,thread_id,name,trip,subject,comment,created_at,capcode,poster_id,json_op_poster_id,
        country,country_name,board_flag,board_flag_type,flag_name,image_spoiler,file_deleted,comment_format,
        staff_authorized_limits,saved_presentation,saved_presentation_search)
    SELECT m.destination_post_id,i.destination_board,i.destination_thread,p.name,p.trip,p.subject,s.comment,p.created_at,
        p.capcode,NULL,NULL,CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country ELSE 'XX' END,
        CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country_name ELSE 'Unknown' END,
        NULL,'pol',NULL,false,p.file_deleted,m.comment_format,m.authorized,s.saved_presentation,s.saved_presentation_search
    FROM post_secrets.thread_transfer_map m JOIN post_secrets.thread_transfer_sealed_rows s
        ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal JOIN content.posts p ON p.id=m.source_post_id
    JOIN content.boards b ON b.slug=i.source_board WHERE m.ticket_hash=p_ticket ORDER BY m.ordinal;
    GET DIAGNOSTICS v_inserted=ROW_COUNT;
    IF v_inserted<>v_count OR EXISTS(
        SELECT 1 FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.thread_transfer_sealed_rows s ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal
        JOIN content.posts p ON p.id=m.source_post_id JOIN content.boards b ON b.slug=i.source_board
        LEFT JOIN content.posts d ON d.id=m.destination_post_id WHERE m.ticket_hash=p_ticket AND (
            d.id IS NULL OR ROW(d.board,d.thread_id,d.name,d.trip,d.subject,d.comment,d.created_at,d.capcode,
                d.poster_id,d.json_op_poster_id,d.country,d.country_name,d.board_flag,d.board_flag_type,d.flag_name,
                d.image_spoiler,d.file_deleted,d.comment_format,d.staff_authorized_limits,d.saved_presentation,
                d.saved_presentation_search,d.wordfilter_payload,d.wordfilter_search,d.drawing_time_seconds,
                d.drawing_source_post_id,d.dice_result,d.fortune_text,d.fortune_color,d.deleted,d.content_erased)
              IS DISTINCT FROM ROW(i.destination_board,i.destination_thread,p.name,p.trip,p.subject,s.comment,
                p.created_at,p.capcode,NULL::text,NULL::text,
                CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country ELSE 'XX' END,
                CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country_name ELSE 'Unknown' END,
                NULL::text,'pol'::text,NULL::text,false,p.file_deleted,m.comment_format,m.authorized,
                s.saved_presentation,s.saved_presentation_search,NULL::bytea,NULL::text,NULL::integer,
                NULL::bigint,NULL::text,NULL::text,NULL::text,false,false))) THEN
        RAISE EXCEPTION 'Transfer payload verification failed.' USING ERRCODE='P131R';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM content.threads t JOIN content.posts p ON p.id=i.source_thread
        WHERE t.id=i.destination_thread AND ROW(t.board,t.created_at,t.bumped_at,t.modified_at,t.http_modified_at,
            t.reply_count,t.sticky,t.sticky_rank,t.closed,t.permasage,t.permaage,t.undead,t.archived_at,
            t.archive_expires_at,t.deleted,t.content_erased)
          IS NOT DISTINCT FROM ROW(i.destination_board,p.created_at,v_now,p.created_at,v_now,v_count-1,
            false,0::smallint,false,false,false,false,NULL::timestamptz,NULL::timestamptz,false,false)) THEN
        RAISE EXCEPTION 'Transfer thread verification failed.' USING ERRCODE='P131R';
    END IF;
    INSERT INTO post_secrets.deletion(post_id,password_hash)
        SELECT m.destination_post_id,d.password_hash FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket;
    INSERT INTO post_secrets.op_peers(thread_id,peer)
        SELECT i.destination_thread,o.peer FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread;
    INSERT INTO post_secrets.op_replies(post_id,thread_id)
        SELECT m.destination_post_id,i.destination_thread FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.op_replies r ON r.post_id=m.source_post_id AND r.thread_id=i.source_thread
        WHERE m.ticket_hash=p_ticket;
    INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at,automatic_identity,registration_xid,automatic_browser,automatic_prefix)
        SELECT m.destination_post_id,i.destination_board,i.destination_thread,h.actor_hash,h.request_at,h.automatic_identity,
            CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid END,h.automatic_browser,h.automatic_prefix
        FROM post_secrets.thread_transfer_map m JOIN post_secrets.posting_history h ON h.post_id=m.source_post_id
            AND h.board=i.source_board AND h.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket;
    v_anonymous_at:=extract(epoch FROM clock_timestamp())::bigint;
    INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
        SELECT m.destination_post_id,a.token_hash,a.password_proof FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.anonymous_posts a ON a.post_id=m.source_post_id
        JOIN post_secrets.anonymous_sessions s ON s.token_hash=a.token_hash AND s.expires_at>v_anonymous_at
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
        WHERE m.ticket_hash=p_ticket;
    INSERT INTO verification_private.post_ownership(post_id,identity_id,password_proof)
        SELECT m.destination_post_id,o.identity_id,o.password_proof FROM post_secrets.thread_transfer_map m
        JOIN verification_private.post_ownership o ON o.post_id=m.source_post_id
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND o.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
        WHERE m.ticket_hash=p_ticket;
    -- Compare complete private row sets, not only counts. These sets and all
    -- hashes/peers/UUIDs remain inside SQL; errors contain no evidence values.
    IF EXISTS(
        WITH expected(kind,id,payload) AS (
            SELECT 'deletion',m.destination_post_id,jsonb_build_array(d.password_hash)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.deletion d ON d.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'peer',i.destination_thread,jsonb_build_array(o.peer)
            FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread
            UNION ALL SELECT 'reply',m.destination_post_id,jsonb_build_array(i.destination_thread)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.op_replies r ON r.post_id=m.source_post_id
                AND r.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'history',m.destination_post_id,jsonb_build_array(i.destination_board,i.destination_thread,
                encode(h.actor_hash,'hex'),h.request_at,h.automatic_identity,
                CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid::text END,
                h.automatic_browser,encode(h.automatic_prefix,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.posting_history h ON h.post_id=m.source_post_id
                AND h.board=i.source_board AND h.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'anonymous',m.destination_post_id,jsonb_build_array(encode(a.token_hash,'hex'),encode(a.password_proof,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.anonymous_posts a ON a.post_id=m.source_post_id
            JOIN post_secrets.anonymous_sessions session ON session.token_hash=a.token_hash AND session.expires_at>v_anonymous_at
            JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
            WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'email-ownership',m.destination_post_id,jsonb_build_array(o.identity_id,encode(o.password_proof,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN verification_private.post_ownership o ON o.post_id=m.source_post_id
            JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND o.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
            WHERE m.ticket_hash=p_ticket
        ), actual(kind,id,payload) AS (
            SELECT 'deletion',d.post_id,jsonb_build_array(d.password_hash) FROM post_secrets.deletion d
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=d.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'peer',o.thread_id,jsonb_build_array(o.peer) FROM post_secrets.op_peers o WHERE o.thread_id=i.destination_thread
            UNION ALL SELECT 'reply',r.post_id,jsonb_build_array(r.thread_id) FROM post_secrets.op_replies r
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=r.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'history',h.post_id,jsonb_build_array(h.board,h.thread_id,encode(h.actor_hash,'hex'),
                h.request_at,h.automatic_identity,h.registration_xid::text,h.automatic_browser,encode(h.automatic_prefix,'hex')) FROM post_secrets.posting_history h
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=h.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'anonymous',a.post_id,jsonb_build_array(encode(a.token_hash,'hex'),encode(a.password_proof,'hex'))
                FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=a.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'email-ownership',o.post_id,jsonb_build_array(o.identity_id,encode(o.password_proof,'hex'))
                FROM verification_private.post_ownership o JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=o.post_id WHERE m.ticket_hash=p_ticket
        ) SELECT 1 FROM ((SELECT * FROM expected EXCEPT ALL SELECT * FROM actual)
            UNION ALL (SELECT * FROM actual EXCEPT ALL SELECT * FROM expected)) different
    ) OR EXISTS(SELECT 1 FROM post_secrets.poster_contexts c JOIN post_secrets.thread_transfer_map m
        ON m.destination_post_id=c.post_id WHERE m.ticket_hash=p_ticket) THEN
        RAISE EXCEPTION 'Transfer evidence verification failed.' USING ERRCODE='P131R';
    END IF;
    UPDATE content.threads SET deleted=true WHERE board=i.source_board AND id=i.source_thread;
    INSERT INTO content.moderation_audit(account_id,board,target_id,action)
        VALUES(i.account_id,i.source_board,i.source_thread,'text-thread-transfer') RETURNING id INTO v_audit;
    INSERT INTO post_secrets.thread_transfer_results(request_id,account_id,source_board,source_thread,
        destination_board,destination_thread,post_count,audit_id)
        VALUES(i.request_id,i.account_id,i.source_board,i.source_thread,i.destination_board,i.destination_thread,v_count,v_audit);
    INSERT INTO post_secrets.thread_transfer_result_posts
        SELECT i.request_id,m.ordinal,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    PERFORM post_secrets.transfer_authority(i.session_hash,i.csrf_hash,i.idle_seconds,
        i.source_board,i.destination_board,true,false,i.credential_id);
    DELETE FROM post_secrets.thread_transfer_active WHERE ticket_hash=p_ticket AND transaction_id=pg_current_xact_id();
    DELETE FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    RETURN QUERY SELECT r.request_id::text,r.source_board,r.source_thread,r.destination_board,r.destination_thread,
        r.post_count::bigint,r.audit_id,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_results r JOIN post_secrets.thread_transfer_result_posts m ON m.request_id=r.request_id
        WHERE r.request_id=i.request_id ORDER BY m.ordinal;
END $$;
CREATE OR REPLACE FUNCTION post_secrets.transfer_post_hash(p_id bigint) RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 SELECT sha256(convert_to(jsonb_build_array(p.id,p.board,p.thread_id,p.name,p.trip,p.subject,p.comment,
    p.created_at,p.deleted,p.content_erased,p.capcode,p.poster_id,p.json_op_poster_id,p.country,p.country_name,
    p.board_flag,p.board_flag_type,p.flag_name,p.image_spoiler,p.file_deleted,p.comment_format,
    p.staff_authorized_limits,encode(p.wordfilter_payload,'hex'),p.wordfilter_search,p.drawing_time_seconds,
    p.drawing_source_post_id,p.dice_result,p.fortune_text,p.fortune_color,
    encode(p.saved_presentation,'hex'),p.saved_presentation_search,
    (SELECT jsonb_build_array(h.automatic_browser,encode(h.automatic_prefix,'hex'))
       FROM post_secrets.posting_history h WHERE h.post_id=p.id AND h.board=p.board))::text,'UTF8'))
 FROM content.posts p WHERE p.id=p_id
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_thread_transfer_owner;
GRANT CREATE ON SCHEMA restrictions TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions' OR c.oid='post_secrets.posting_history'::regclass
 ), triggers AS (
  SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'post_secrets.posting_history'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='post_secrets.posting_history'::regclass AND t.tgname='source_restriction_guard' AND NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'content.reports'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='content.reports'::regclass AND t.tgname='source_report_restriction_guard' AND NOT t.tgisinternal
 ), funcs AS (
  SELECT p.* FROM pg_proc p WHERE p.proowner='board_restriction_owner'::regrole
    OR p.oid IN(SELECT t.tgfoid FROM triggers t)
    OR p.oid IN('content.record_inserted_posting_history()'::regprocedure,
       'content.lock_report_network_policy()'::regprocedure,
       'content.report_network_rules(text,bigint,boolean)'::regprocedure,
       'post_secrets.report_network_activity(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
       'post_secrets.assert_report_network(text,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,restrictions.acl(coalesce(r.relacl,acldefault('r',r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
       pg_get_expr(d.adbin,d.adrelid),restrictions.acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated)
       ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
    ORDER BY t.identity,t.tgname) FROM triggers t),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),pg_get_functiondef(p.oid),
      has_function_privilege('board_restriction_owner',p.oid,'EXECUTE')) ORDER BY p.oid::regprocedure::text)
   FROM pg_proc p WHERE p.oid IN('staff_identity.has_board_access(bigint,text)'::regprocedure,
      'content.lock_posting_actor(bytea,boolean)'::regprocedure,
      'post_secrets.lookup_automatic_identity(bytea,bigint)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
      'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),restrictions.acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
   FROM pg_namespace n WHERE n.nspname='restrictions'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions FROM board_restriction_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Automatic metadata did not preserve transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Automatic metadata did not preserve restriction authority'; END IF; END $$;
RESET ROLE;
