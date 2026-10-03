-- Unbadged staff posts retain public metadata, admission and deletion behavior.
-- A private single-use proof binds derived identity to current staff authority.
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN ordinary boolean NOT NULL DEFAULT false;
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN ordinary_context jsonb;
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN ordinary_policy jsonb;
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_capcode_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_capcode_check CHECK (
    (ordinary AND board<>'j' AND capcode='none')
    OR (NOT ordinary AND (capcode IN ('mod','admin','admin_highlight','manager','developer','founder')
        OR (board='j' AND capcode='janitor'))));
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_source_identity_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_source_identity_check CHECK (
    (NOT ordinary AND ordinary_context IS NULL AND ordinary_policy IS NULL AND (
        (source_options IS NULL AND prepared_trip IS NULL AND source_name_allowed IS NULL)
        OR (source_options IN ('capcode_mod','capcode_dev','capcode_manager','capcode_admin','capcode_founder','capcode_admin_hl')
            AND source_options IS NOT NULL AND source_name_allowed IS NOT NULL
            AND (prepared_trip IS NULL OR prepared_trip ~ '^(![./A-Za-z0-9]{10}|!![A-Za-z0-9+/]{11})$')
            AND (source_name_allowed OR (name='Anonymous' AND prepared_trip IS NULL)))))
    OR (ordinary AND source_options IS NOT NULL AND octet_length(source_options)<=100
        AND source_options !~ '[sS][aA][gG][eE]' AND source_name_allowed IS NOT NULL
        AND ordinary_context IS NOT NULL AND jsonb_typeof(ordinary_context)='object'
        AND octet_length(ordinary_context::text)<=4096 AND ordinary_policy IS NOT NULL
        AND (prepared_trip IS NULL OR prepared_trip ~ '^(![./A-Za-z0-9]{10}|!![A-Za-z0-9+/]{11})$')
        AND (source_name_allowed OR (name='Anonymous' AND prepared_trip IS NULL))));

GRANT UPDATE(ordinary,ordinary_context,ordinary_policy) ON post_secrets.staff_post_intents TO board_staff_post_owner;
GRANT SELECT(staff_only,user_ids,meta_board,poster_id_no_heaven,country_flags,board_flags,robot9000,
    op_markup,dice_roll,fortune_trip,word_filter_enabled,word_filter_profile)
    ON content.boards TO board_staff_post_owner;
GRANT SELECT(id,board,thread_id,deleted,created_at) ON content.posts TO board_staff_post_owner;
GRANT SELECT(id,board,deleted,archived_at) ON content.threads TO board_staff_post_owner;
GRANT SELECT,INSERT ON post_secrets.op_peers,post_secrets.op_replies,post_secrets.deletion TO board_staff_post_owner;
GRANT CREATE ON SCHEMA content,staff_identity TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;


-- A caller supplies its resolved peer. Only membership and the latest own
-- reply time leave the private schema; no runtime receives bulk peer access.
CREATE FUNCTION content.staff_op_context(v_board text,v_thread bigint,v_peer text)
RETURNS TABLE(own_reply boolean,latest_reply timestamptz)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    SELECT EXISTS(SELECT 1 FROM post_secrets.op_peers o WHERE o.thread_id=t.id AND o.peer=v_peer::inet),
        (SELECT p.created_at FROM post_secrets.op_replies r JOIN content.posts p ON p.id=r.post_id
            WHERE r.thread_id=t.id AND p.board=v_board AND p.thread_id=t.id AND NOT p.deleted
            ORDER BY p.id DESC LIMIT 1)
    FROM content.threads t JOIN content.boards b ON b.slug=t.board
    WHERE t.board=v_board AND t.id=v_thread AND NOT t.deleted AND t.archived_at IS NULL AND NOT b.staff_only
$$;

CREATE FUNCTION content.staff_op_deletion_hash(v_board text,v_thread bigint) RETURNS text
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    SELECT d.password_hash FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id
    JOIN content.threads t ON t.id=p.thread_id JOIN content.boards b ON b.slug=p.board
    WHERE p.board=v_board AND p.id=v_thread AND p.thread_id=p.id AND NOT p.deleted
        AND NOT t.deleted AND t.archived_at IS NULL AND NOT b.staff_only
$$;

CREATE FUNCTION content.check_staff_op_context(v_board text,v_thread bigint,v_post bigint,v_context jsonb)
RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE eligible boolean; enabled boolean; own boolean;
BEGIN
    SELECT op_markup INTO enabled FROM content.boards WHERE slug=v_board AND NOT staff_only;
    SELECT own_reply INTO own FROM content.staff_op_context(v_board,v_thread,v_context->>'peer');
    eligible:=coalesce(enabled,false) AND (v_thread=v_post OR coalesce(own,false) OR
        (v_context->>'op_password_proof'<>'' AND v_context->>'op_password_proof'=
            encode(sha256(convert_to(content.staff_op_deletion_hash(v_board,v_thread),'UTF8')),'hex')));
    IF (v_context->>'source_op_reply')::boolean IS DISTINCT FROM coalesce(eligible,false) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
END $$;

-- This trigger runs after the proof-consuming BEFORE trigger. Its marker is
-- reset for every insert, including legacy badge and private discussion posts.
CREATE FUNCTION content.record_ordinary_staff_secrets() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF current_setting('board.staff_ordinary_post',true)='true' THEN
        INSERT INTO post_secrets.deletion(post_id,password_hash)
            VALUES(NEW.id,current_setting('board.deletion_hash',true));
        IF NEW.id=NEW.thread_id THEN
            INSERT INTO post_secrets.op_peers(thread_id,peer)
                VALUES(NEW.id,current_setting('board.peer',true)::inet);
        ELSIF EXISTS(SELECT 1 FROM post_secrets.op_peers WHERE thread_id=NEW.thread_id
            AND peer=current_setting('board.peer',true)::inet) THEN
            INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES(NEW.id,NEW.thread_id);
        END IF;
    END IF;
    RETURN NEW;
END $$;

CREATE FUNCTION staff_identity.discard_ordinary_post_authority(ticket bytea,session_token bytea)
RETURNS void LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    DELETE FROM post_secrets.staff_post_intents
    WHERE token_hash=ticket AND session_hash=session_token AND ordinary
$$;

CREATE FUNCTION content.staff_ordinary_context() RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
    SELECT jsonb_build_object(
        'poster_id',coalesce(current_setting('board.poster_id',true),''),
        'poster_fingerprint',coalesce(current_setting('board.poster_fingerprint',true),''),
        'poster_epoch',coalesce(current_setting('board.poster_epoch',true),''),
        'post_sage',coalesce(current_setting('board.post_sage',true),''),
        'country',coalesce(current_setting('board.country',true),''),
        'country_name',coalesce(current_setting('board.country_name',true),''),
        'flag',coalesce(current_setting('board.flag',true),''),
        'source_op_reply',coalesce(current_setting('board.source_op_reply',true),''),
        'dice_result',coalesce(current_setting('board.dice_result',true),''),
        'fortune_text',coalesce(current_setting('board.fortune_text',true),''),
        'fortune_color',coalesce(current_setting('board.fortune_color',true),''),
        'peer',coalesce(current_setting('board.peer',true),''),
        'deletion_hash',coalesce(current_setting('board.deletion_hash',true),''),
        'op_password_proof',coalesce(current_setting('board.op_password_proof',true),''))
$$;

CREATE FUNCTION content.staff_ordinary_policy(v_board text) RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
    SELECT jsonb_build_object('user_ids',user_ids,'meta_board',meta_board,
        'no_heaven',poster_id_no_heaven,'country_flags',country_flags,'board_flags',board_flags,
        'forced_anon',forced_anon,'strip_tripcode',strip_tripcode,'robot9000',robot9000,
        'op_markup',op_markup,'dice_roll',dice_roll,'fortune_trip',fortune_trip,
        'word_filter_enabled',word_filter_enabled,'word_filter_profile',word_filter_profile)
    FROM content.boards WHERE slug=v_board AND NOT staff_only
$$;

CREATE FUNCTION staff_identity.check_ordinary_post_identity(
    v_role text,v_flags text[],v_allow text[],v_deny text[],v_options text,
    v_name text,v_trip text,v_name_allowed boolean,v_subject text,v_policy jsonb,
    v_context jsonb,v_authorized boolean
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE named boolean; label text; item record; has_country boolean;
BEGIN
    IF v_role NOT IN ('janitor','moderator','manager','admin') OR v_role IS NULL
        OR v_options IS NULL OR octet_length(v_options)>100 OR v_options ~ '[sS][aA][gG][eE]'
        OR v_policy IS NULL OR v_context IS NULL OR jsonb_typeof(v_context)<>'object'
        OR octet_length(v_context::text)>4096 OR v_name_allowed IS NULL
        OR v_authorized IS DISTINCT FROM (v_role<>'janitor') THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF left(v_options,8)='capcode_' THEN
        label:=staff_identity.source_public_capcode(v_role,v_flags,v_allow,v_deny,v_options);
        named:=staff_identity.source_capcode_name_allowed(v_role,v_flags,v_allow,v_deny);
    ELSE label:='none'; named:=true; END IF;
    IF label IS DISTINCT FROM 'none' OR named IS DISTINCT FROM v_name_allowed
        OR (NOT named AND (v_name<>'Anonymous' OR v_trip IS NOT NULL))
        OR (v_trip IS NOT NULL AND v_trip !~ '^(![./A-Za-z0-9]{10}|!![A-Za-z0-9+/]{11})$')
        OR content.staff_display_name_size(v_name,v_trip)>(CASE WHEN v_authorized THEN 255 ELSE 100 END)
        OR (v_name='' AND v_trip IS NULL)
        OR ((v_policy->>'forced_anon')::boolean AND v_role<>'admin' AND (v_name<>'Anonymous' OR v_trip IS NOT NULL))
        OR ((v_policy->>'forced_anon')::boolean AND v_subject<>'')
        OR ((v_policy->>'strip_tripcode')::boolean AND v_trip IS NOT NULL) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF (SELECT count(*) FROM jsonb_object_keys(v_context))<>14 THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    FOR item IN SELECT key,value FROM jsonb_each(v_context) LOOP
        IF item.key NOT IN ('poster_id','poster_fingerprint','poster_epoch','post_sage','country',
            'country_name','flag','source_op_reply','dice_result','fortune_text','fortune_color','peer','deletion_hash','op_password_proof')
            OR jsonb_typeof(item.value)<>'string' THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
    END LOOP;
    IF v_context->>'post_sage' NOT IN ('true','false') OR v_context->>'source_op_reply' NOT IN ('true','false')
        OR ((v_policy->>'user_ids')::boolean AND v_context->>'poster_id' !~ '^[+/0-9A-Za-z]{8}$')
        OR (NOT (v_policy->>'user_ids')::boolean AND v_context->>'poster_id'<>'')
        OR ((v_context->>'poster_fingerprint'='')<>(v_context->>'poster_epoch'=''))
        OR (v_context->>'poster_fingerprint'<>'' AND (
            v_context->>'poster_fingerprint' !~ '^[0-9a-f]{64}$' OR v_context->>'poster_epoch' !~ '^[0-9a-f]{64}$'))
        OR (v_context->>'flag'<>'' AND NOT (v_policy->'board_flags' ? (v_context->>'flag')))
        OR (v_context->>'source_op_reply'='true' AND NOT (v_policy->>'op_markup')::boolean) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    has_country:=(v_policy->>'country_flags')::boolean AND v_context->>'flag'='';
    IF has_country THEN
        IF v_context->>'country' !~ '^[A-Z]{2}$' OR octet_length(v_context->>'country_name') NOT BETWEEN 1 AND 100
            OR v_context->>'country_name' ~ '[[:cntrl:]]' THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
    ELSIF v_context->>'country'<>'' OR v_context->>'country_name'<>'' THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF octet_length(v_context->>'peer') NOT BETWEEN 1 AND 45
        OR host((v_context->>'peer')::inet) IS DISTINCT FROM v_context->>'peer'
        OR masklen((v_context->>'peer')::inet)<>(CASE family((v_context->>'peer')::inet) WHEN 4 THEN 32 ELSE 128 END)
        OR v_context->>'poster_fingerprint'='' OR v_context->>'poster_epoch'=''
        OR octet_length(v_context->>'deletion_hash') NOT BETWEEN 1 AND 256
        OR (v_context->>'op_password_proof'<>'' AND v_context->>'op_password_proof' !~ '^[0-9a-f]{64}$') THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    -- Release-owned randomizer validation remains in the existing content
    -- triggers. The proof must bind even an empty result, preventing a later
    -- caller from introducing dice or fortune output after issuance.
    IF (v_context->>'dice_result'<>'' AND NOT (v_policy->>'dice_roll')::boolean)
        OR ((v_context->>'fortune_text'<>'' OR v_context->>'fortune_color'<>'')
            AND NOT (v_policy->>'fortune_trip')::boolean)
        OR octet_length(v_context->>'dice_result')>1024 OR octet_length(v_context->>'fortune_text')>256
        OR octet_length(v_context->>'fortune_color')>32 THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
END $$;

CREATE FUNCTION staff_identity.issue_ordinary_post_authority(
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
        OR substring(v_wordfilter_payload FROM 1 FOR 4)<>decode(CASE WHEN v_authorized THEN '57463032' ELSE '57463031' END,'hex')) THEN
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

ALTER FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)
    RENAME TO consume_badged_post_authority;
REVOKE ALL ON FUNCTION content.consume_badged_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)
    FROM PUBLIC,board_staff;

CREATE FUNCTION content.consume_staff_post_authority(ticket bytea,post_number bigint,v_board text,
    v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE intent post_secrets.staff_post_intents%ROWTYPE; locked_intent post_secrets.staff_post_intents%ROWTYPE;
    actual_role text; flags text[]; allow_boards text[]; deny_boards text[]; expected_limit integer; item record;
BEGIN
    PERFORM set_config('board.staff_ordinary_post','false',true);
    SELECT * INTO intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
    IF NOT intent.ordinary THEN
        RETURN content.consume_badged_post_authority(ticket,post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time);
    END IF;
    IF intent.post_id IS DISTINCT FROM post_number OR intent.board IS DISTINCT FROM v_board
        OR intent.thread_id IS DISTINCT FROM v_thread OR intent.name IS DISTINCT FROM v_name
        OR intent.subject IS DISTINCT FROM v_subject OR intent.comment IS DISTINCT FROM v_comment
        OR intent.posted_at IS DISTINCT FROM v_time
        OR intent.wordfilter_payload IS DISTINCT FROM decode(nullif(current_setting('board.wordfilter_payload',true),''),'hex')
        OR intent.wordfilter_search IS DISTINCT FROM (CASE WHEN intent.wordfilter_payload IS NOT NULL THEN current_setting('board.wordfilter_search',true) ELSE NULL END)
        OR intent.prepared_trip IS DISTINCT FROM nullif(current_setting('board.post_trip',true),'')
        OR intent.ordinary_context IS DISTINCT FROM content.staff_ordinary_context()
        OR intent.source_options IS DISTINCT FROM coalesce(current_setting('board.staff_post_options',true),'') THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT CASE WHEN intent.authorized_limits THEN max_authorized_comment_chars ELSE max_comment_chars END
        INTO expected_limit FROM content.boards WHERE slug=v_board AND NOT staff_only;
    IF expected_limit IS DISTINCT FROM intent.comment_limit
        OR intent.ordinary_policy IS DISTINCT FROM content.staff_ordinary_policy(v_board) THEN
        RAISE EXCEPTION 'Staff posting policy changed' USING ERRCODE='28000';
    END IF;
    SELECT a.role,a.flags,a.allow_boards,a.deny_boards INTO actual_role,flags,allow_boards,deny_boards
        FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id
        AND s.expires_at>clock_timestamp() AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes' AND a.revoked_at IS NULL
        AND a.role IN ('janitor','moderator','manager','admin') AND staff_identity.has_board_access(a.id,v_board)
        FOR SHARE OF a,s;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
    PERFORM staff_identity.check_ordinary_post_identity(actual_role,flags,allow_boards,deny_boards,
        intent.source_options,intent.name,intent.prepared_trip,intent.source_name_allowed,
        intent.subject,intent.ordinary_policy,intent.ordinary_context,intent.authorized_limits);
    PERFORM content.check_staff_op_context(v_board,v_thread,post_number,intent.ordinary_context);
    SELECT * INTO locked_intent FROM post_secrets.staff_post_intents WHERE token_hash=ticket FOR UPDATE;
    IF NOT FOUND OR locked_intent IS DISTINCT FROM intent OR intent.expires_at<=clock_timestamp()
        OR NOT EXISTS(SELECT 1 FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
            WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id
            AND s.expires_at>clock_timestamp() AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
            AND s.authenticated_at>clock_timestamp()-interval '10 minutes' AND a.revoked_at IS NULL
            AND a.role=actual_role AND staff_identity.has_board_access(a.id,v_board)) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    PERFORM set_config('board.staff_authorized_limits',intent.authorized_limits::text,true),
        set_config('board.staff_is_admin',(actual_role='admin')::text,true),
        set_config('board.staff_ordinary_post','true',true),
        set_config('board.post_trip',coalesce(intent.prepared_trip,''),true);
    FOR item IN SELECT key,value FROM jsonb_each_text(intent.ordinary_context) LOOP
        PERFORM set_config('board.'||item.key,item.value,true);
    END LOOP;
    DELETE FROM post_secrets.staff_post_intents WHERE token_hash=ticket;
    INSERT INTO content.moderation_audit(account_id,board,target_id,action)
        VALUES(intent.account_id,v_board,post_number,'staff-post');
    RETURN NULL;
END $$;

REVOKE ALL ON FUNCTION content.staff_ordinary_context(),content.staff_ordinary_policy(text),
    staff_identity.check_ordinary_post_identity(text,text[],text[],text[],text,text,text,boolean,text,jsonb,jsonb,boolean),
    staff_identity.issue_ordinary_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb),
    content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
    staff_identity.issue_ordinary_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb) TO board_auth;
GRANT EXECUTE ON FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz),
    content.staff_ordinary_context() TO board_staff;
REVOKE ALL ON FUNCTION content.staff_op_context(text,bigint,text),content.staff_op_deletion_hash(text,bigint),
    content.check_staff_op_context(text,bigint,bigint,jsonb),content.record_ordinary_staff_secrets(),
    staff_identity.discard_ordinary_post_authority(bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.staff_op_context(text,bigint,text),content.staff_op_deletion_hash(text,bigint) TO board_staff;
GRANT EXECUTE ON FUNCTION staff_identity.discard_ordinary_post_authority(bytea,bytea) TO board_auth;
GRANT EXECUTE ON FUNCTION content.record_ordinary_staff_secrets() TO board_migrator;
RESET ROLE;
CREATE TRIGGER record_ordinary_staff_secrets AFTER INSERT ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.record_ordinary_staff_secrets();
SET LOCAL ROLE board_staff_post_owner;
REVOKE EXECUTE ON FUNCTION content.record_ordinary_staff_secrets() FROM board_migrator;
RESET ROLE;
SET LOCAL ROLE board_admission_owner;
GRANT EXECUTE ON FUNCTION content.lock_content_admission(text,text),content.content_admission_rules(text),
    content.record_content_admission(text,text,bigint,bigint,text,bigint,text,text,text,text) TO board_staff;
RESET ROLE;
SET LOCAL ROLE board_robot9000_owner;
GRANT EXECUTE ON FUNCTION content.check_robot9000(text,bytea,bytea,double precision,timestamptz) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,staff_identity FROM board_staff_post_owner;

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
