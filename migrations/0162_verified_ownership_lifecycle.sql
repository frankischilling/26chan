-- Keep verified deletion ownership when a staff-authorized text thread moves,
-- and erase that proof with the post's other private content on whole deletion.
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Existing transfer catalog has drifted'; END IF; END $$;
RESET ROLE;
GRANT USAGE ON SCHEMA verification_private TO board_thread_transfer_owner;
GRANT SELECT,INSERT,UPDATE(post_id) ON verification_private.post_ownership TO board_thread_transfer_owner;
GRANT CREATE ON SCHEMA content,post_secrets TO board_thread_transfer_owner;
SET ROLE board_thread_transfer_owner;
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
    INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at,automatic_identity,registration_xid)
        SELECT m.destination_post_id,i.destination_board,i.destination_thread,h.actor_hash,h.request_at,h.automatic_identity,
            CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid END
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
                CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid::text END)
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
                h.request_at,h.automatic_identity,h.registration_xid::text) FROM post_secrets.posting_history h
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

CREATE OR REPLACE FUNCTION post_secrets.transfer_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
    SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE c.relkind='r' AND (
      (n.nspname='post_secrets' AND (c.relname LIKE 'thread_transfer_%'
        OR c.relname IN('deletion','op_peers','op_replies','posting_history','anonymous_posts','anonymous_sessions','poster_contexts')))
      OR (n.nspname='content' AND c.relname IN('boards','threads','posts','post_media','moderation_audit'))
      OR (n.nspname='verification_private' AND c.relname='post_ownership')
      OR (n.nspname='staff_identity' AND c.relname IN('accounts','sessions','credentials')))
 ), triggers AS (
    SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
 ), funcs AS (
    SELECT p.* FROM pg_proc p WHERE p.proowner='board_thread_transfer_owner'::regrole
      OR p.oid IN(SELECT t.tgfoid FROM triggers t)
      OR p.oid IN('content.erasure_visibility_ready()'::regprocedure,
        'staff_identity.has_board_access(bigint,text)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
    (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner)::text,
       r.relkind,r.relpersistence,r.relrowsecurity,r.relforcerowsecurity,
       post_secrets.transfer_acl(coalesce(r.relacl,acldefault('r',r.relowner))),
       (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,
          a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid),post_secrets.transfer_acl(a.attacl)) ORDER BY a.attnum)
        FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
        WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
       (SELECT jsonb_agg(jsonb_build_array(k.conname,k.contype,k.convalidated,k.condeferrable,k.condeferred,
           pg_get_constraintdef(k.oid,true)) ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid),
       (SELECT jsonb_agg(jsonb_build_array(p.polname,p.polcmd,p.polpermissive,
          (SELECT jsonb_agg(CASE WHEN x=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x)::text END
             ORDER BY CASE WHEN x=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x)::text END) FROM unnest(p.polroles) x),
          pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)) ORDER BY p.polname)
        FROM pg_policy p WHERE p.polrelid=r.oid)) ORDER BY r.identity) FROM relations r),
    (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
       ORDER BY t.identity,t.tgname) FROM triggers t),
    (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner)::text,
       post_secrets.transfer_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
       ORDER BY p.oid::regprocedure::text) FROM funcs p))::text,'UTF8'))
$$;

CREATE OR REPLACE FUNCTION content.text_thread_transfer_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 WITH owner_role AS (
    SELECT oid FROM pg_roles WHERE rolname='board_thread_transfer_owner'
        AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)
 ), required(schema_name,table_name,rls,policy_name) AS (VALUES
    ('content','boards',true,'board_visibility'),('content','posts',true,'post_visibility'),
    ('content','threads',true,'thread_visibility'),('content','post_media',false,''),
    ('post_secrets','deletion',true,'deletion_visibility'),('post_secrets','op_peers',true,'op_peer_visibility'),
    ('post_secrets','op_replies',true,'op_reply_visibility'),('post_secrets','posting_history',false,''),
    ('post_secrets','anonymous_posts',false,''),('post_secrets','anonymous_sessions',false,''),
    ('post_secrets','poster_contexts',false,''),('verification_private','post_ownership',false,''),
    ('staff_identity','accounts',false,''),('staff_identity','sessions',false,''),('staff_identity','credentials',false,'')
 ), relations AS (
    SELECT c.*,r.rls,r.policy_name FROM required r JOIN pg_namespace n ON n.nspname=r.schema_name
       JOIN pg_class c ON c.relnamespace=n.oid AND c.relname=r.table_name
 )
 SELECT EXISTS(SELECT 1 FROM owner_role)
    AND content.erasure_visibility_ready()
    AND (SELECT count(*) FROM relations)=15
    AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.relrowsecurity<>r.rls OR r.relforcerowsecurity
        OR r.relkind<>'r' OR r.relpersistence<>'p')
    AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.rls AND NOT EXISTS(
        SELECT 1 FROM pg_policy p WHERE p.polrelid=r.oid AND p.polname=r.policy_name
          AND p.polroles=ARRAY[0]::oid[] AND p.polcmd='*' AND p.polpermissive AND p.polwithcheck IS NULL))
    -- In particular no transfer-owner restrictive policy may hide replies or
    -- private evidence that canonical erasure would subsequently remove.
    AND NOT EXISTS(SELECT 1 FROM relations r JOIN pg_policy p ON p.polrelid=r.oid
        WHERE (p.polname<>r.policy_name OR NOT p.polpermissive) AND EXISTS(
          SELECT 1 FROM unnest(p.polroles) role_id WHERE role_id=0
             OR pg_has_role((SELECT oid FROM owner_role),role_id,'USAGE')))
    AND EXISTS(SELECT 1 FROM pg_policy p WHERE p.polrelid='content.boards'::regclass AND p.polname='board_visibility'
        AND lower(regexp_replace(pg_get_expr(p.polqual,p.polrelid),'[[:space:]()]','','g'))=
            'notstaff_onlyorcurrent_user=anyarray[''board_staff''::name,''board_migrator''::name]')
    AND NOT EXISTS(SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member
        WHERE m.roleid=(SELECT oid FROM owner_role) AND r.rolname<>'board_migrator')
    AND NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.member=(SELECT oid FROM owner_role))
    AND has_schema_privilege('board_thread_transfer_owner','verification_private','USAGE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','verification_private','CREATE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','content','CREATE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','post_secrets','CREATE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','staff_identity','CREATE')
    AND (SELECT definition_hash=post_secrets.transfer_catalog_hash()
         FROM post_secrets.thread_transfer_catalog WHERE singleton)
    AND NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
        JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
        CROSS JOIN (VALUES('board_public'),('board_staff'),('board_auth'),('board_media'),
            ('board_media_read'),('board_media_intake'),('board_monitor'),('board_verifier'),('board_mailer')) runtime(name)
        WHERE n.nspname='post_secrets' AND c.relname LIKE 'thread_transfer_%'
            AND (has_column_privilege(runtime.name,c.oid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES')
                OR has_table_privilege(runtime.name,c.oid,'DELETE,TRUNCATE,TRIGGER')))
    AND NOT EXISTS(SELECT 1 FROM pg_constraint c JOIN pg_class r ON r.oid=c.conrelid
        JOIN pg_namespace n ON n.oid=r.relnamespace JOIN pg_class target ON target.oid=c.confrelid
        JOIN pg_namespace tn ON tn.oid=target.relnamespace
        WHERE n.nspname='post_secrets' AND r.relname LIKE 'thread_transfer_%' AND c.contype='f'
            AND (tn.nspname<>'post_secrets' OR target.relname NOT IN('thread_transfer_intents','thread_transfer_map')))
$$;

RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_thread_transfer_owner;
GRANT CREATE ON SCHEMA verification_private TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION verification_private.erase_post_ownership() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    DELETE FROM verification_private.post_ownership WHERE post_id=NEW.id;
    RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION verification_private.erase_post_ownership() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION verification_private.erase_post_ownership() TO board_migrator;
RESET ROLE;
CREATE TRIGGER erase_verified_ownership AFTER UPDATE ON content.posts
    FOR EACH ROW WHEN(NOT OLD.content_erased AND NEW.content_erased)
    EXECUTE FUNCTION verification_private.erase_post_ownership();
SET ROLE board_anonymous_owner;
REVOKE EXECUTE ON FUNCTION verification_private.erase_post_ownership() FROM board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private FROM board_anonymous_owner;
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Verified ownership transfer catalog did not qualify'; END IF; END $$;
RESET ROLE;
