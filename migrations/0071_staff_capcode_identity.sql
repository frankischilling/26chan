-- Source badge/name permissions and proof-bound prepared tripcodes.
-- Existing posts and proof payloads remain unchanged.
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN source_options text;
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN prepared_trip text;
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN source_name_allowed boolean;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_source_identity_check CHECK (
    (source_options IS NULL AND prepared_trip IS NULL AND source_name_allowed IS NULL)
    OR (source_options IN ('capcode_mod','capcode_dev','capcode_manager','capcode_admin','capcode_founder','capcode_admin_hl')
        AND source_options IS NOT NULL AND source_name_allowed IS NOT NULL
        AND (prepared_trip IS NULL OR prepared_trip ~ '^(![./A-Za-z0-9]{10}|!![A-Za-z0-9+/]{11})$')
        AND (source_name_allowed OR (name='Anonymous' AND prepared_trip IS NULL))));
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_name_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_name_check CHECK (
    octet_length(name)<=255 AND (name<>'' OR prepared_trip IS NOT NULL));
GRANT UPDATE(name,capcode,source_options,prepared_trip,source_name_allowed)
    ON post_secrets.staff_post_intents TO board_staff_post_owner;
GRANT SELECT(forced_anon,strip_tripcode) ON content.boards TO board_staff_post_owner;

GRANT CREATE ON SCHEMA staff_identity,content TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;

CREATE FUNCTION staff_identity.source_public_capcode(
    v_role text,v_flags text[],v_allow text[],v_deny text[],v_options text
) RETURNS text
LANGUAGE plpgsql IMMUTABLE SET search_path=pg_catalog,pg_temp AS $$
DECLARE rank integer; global_access boolean;
BEGIN
    rank:=CASE v_role WHEN 'janitor' THEN 1 WHEN 'mod' THEN 10 WHEN 'moderator' THEN 10
        WHEN 'manager' THEN 20 WHEN 'admin' THEN 50 ELSE 0 END;
    IF rank<10 OR v_options IS NULL THEN RETURN 'none'; END IF;
    global_access:=coalesce(('all'=ANY(v_allow) OR ''=ANY(v_allow)) AND NOT ('noboard'=ANY(v_deny)),false);
    IF rank=50 AND v_options='capcode_founder' THEN RETURN 'founder'; END IF;
    IF rank=50 AND v_options='capcode_admin' THEN RETURN 'admin'; END IF;
    IF rank=50 AND v_options='capcode_admin_hl' THEN RETURN 'admin_highlight'; END IF;
    IF global_access AND coalesce('developer'=ANY(v_flags),false) AND v_options='capcode_dev' THEN RETURN 'developer'; END IF;
    IF rank>=20 AND v_options='capcode_manager' THEN RETURN 'manager'; END IF;
    IF rank<20 AND NOT (global_access AND coalesce('capcode'=ANY(v_flags),false)) AND v_options<>'' THEN
        RETURN 'cant_capcode';
    END IF;
    RETURN CASE WHEN v_options='capcode_mod' THEN 'mod' ELSE 'none' END;
END $$;

CREATE FUNCTION staff_identity.source_capcode_name_allowed(
    v_role text,v_flags text[],v_allow text[],v_deny text[]
) RETURNS boolean
LANGUAGE sql IMMUTABLE SET search_path=pg_catalog,pg_temp AS $$
    SELECT coalesce(v_role='admin' OR (
        ('all'=ANY(v_allow) OR ''=ANY(v_allow)) AND NOT ('noboard'=ANY(v_deny))
        AND 'capcodename'=ANY(v_flags)),false)
$$;

CREATE FUNCTION content.staff_display_name_size(v_name text,v_trip text) RETURNS integer
LANGUAGE sql IMMUTABLE SET search_path=pg_catalog,pg_temp AS $$
    SELECT octet_length(replace(replace(replace(replace(replace(v_name,'&','&amp;'),'<','&lt;'),
        '>','&gt;'),chr(34),'&quot;'),chr(39),'&#039;'))
        + CASE WHEN v_trip IS NULL THEN 0 ELSE octet_length('</span> <span class="postertrip">'||v_trip) END
$$;
REVOKE ALL ON FUNCTION staff_identity.source_public_capcode(text,text[],text[],text[],text),
    staff_identity.source_capcode_name_allowed(text,text[],text[],text[]),
    content.staff_display_name_size(text,text) FROM PUBLIC;

CREATE FUNCTION staff_identity.issue_source_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,highlight boolean,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_authorized boolean,v_comment_limit integer,v_wordfilter_payload bytea,v_wordfilter_search text,
    v_options text,v_trip text,v_name_allowed boolean
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_role text; v_flags text[]; v_allow text[]; v_deny text[];
    label text; named boolean; v_forced boolean; v_strip boolean;
BEGIN
    IF v_board='j' OR v_options IS NULL OR v_options NOT IN
        ('capcode_mod','capcode_dev','capcode_manager','capcode_admin','capcode_founder','capcode_admin_hl')
        OR highlight IS DISTINCT FROM false OR v_authorized IS DISTINCT FROM true OR v_name_allowed IS NULL
        OR (v_trip IS NOT NULL AND v_trip !~ '^(![./A-Za-z0-9]{10}|!![A-Za-z0-9+/]{11})$')
        OR content.staff_display_name_size(v_name,v_trip)>255
        OR (v_name='' AND v_trip IS NULL) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    -- The existing issuer locks account/session and binds body, policy and
    -- formatter data. Its temporary nonempty name stays in this statement.
    PERFORM staff_identity.issue_limited_post_authority(ticket,session_token,csrf,idle,false,
        post_number,v_board,v_thread,CASE WHEN v_name='' THEN 'Anonymous' ELSE v_name END,
        v_subject,v_comment,v_time,v_authorized,v_comment_limit,v_wordfilter_payload,v_wordfilter_search);
    SELECT a.role,a.flags,a.allow_boards,a.deny_boards INTO v_role,v_flags,v_allow,v_deny
        FROM staff_identity.accounts a JOIN post_secrets.staff_post_intents i
        ON i.account_id=a.id WHERE i.token_hash=ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
    label:=staff_identity.source_public_capcode(v_role,v_flags,v_allow,v_deny,v_options);
    named:=staff_identity.source_capcode_name_allowed(v_role,v_flags,v_allow,v_deny);
    SELECT b.forced_anon,b.strip_tripcode INTO v_forced,v_strip FROM content.boards b WHERE b.slug=v_board;
    IF NOT FOUND OR label NOT IN ('mod','developer','manager','admin','founder','admin_highlight')
        OR named IS DISTINCT FROM v_name_allowed
        OR (NOT named AND (v_name<>'Anonymous' OR v_trip IS NOT NULL))
        OR (v_forced AND v_role<>'admin' AND (v_name<>'Anonymous' OR v_trip IS NOT NULL))
        OR (v_forced AND v_subject<>'') OR (v_strip AND v_trip IS NOT NULL) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    UPDATE post_secrets.staff_post_intents SET name=v_name,capcode=label,source_options=v_options,
        prepared_trip=v_trip,source_name_allowed=named WHERE token_hash=ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
END $$;
REVOKE ALL ON FUNCTION staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean) TO board_auth;

CREATE OR REPLACE FUNCTION content.consume_staff_post_authority(ticket bytea,post_number bigint,v_board text,
    v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE intent post_secrets.staff_post_intents%ROWTYPE; locked_intent post_secrets.staff_post_intents%ROWTYPE; label text; v_role text; expected_limit integer;
    v_flags text[]; v_allow text[]; v_deny text[]; named boolean; chosen text;
    v_forced boolean; v_strip boolean; options text;
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
        OR intent.wordfilter_search IS DISTINCT FROM (CASE WHEN intent.wordfilter_payload IS NOT NULL THEN current_setting('board.wordfilter_search',true) ELSE NULL END)
        OR (intent.source_options IS NOT NULL AND intent.prepared_trip IS DISTINCT FROM nullif(current_setting('board.post_trip',true),'')) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT CASE WHEN intent.authorized_limits THEN b.max_authorized_comment_chars ELSE b.max_comment_chars END,b.forced_anon,b.strip_tripcode
      INTO expected_limit,v_forced,v_strip FROM content.boards b WHERE b.slug=v_board;
    IF NOT FOUND OR (intent.comment_limit IS NOT NULL AND intent.comment_limit IS DISTINCT FROM expected_limit) THEN
        RAISE EXCEPTION 'Staff posting policy changed' USING ERRCODE='28000';
    END IF;
    SELECT CASE WHEN v_board='j' THEN (CASE a.role WHEN 'moderator' THEN 'mod' ELSE a.role END) ELSE coalesce(a.public_capcode,CASE a.role WHEN 'admin' THEN 'admin' WHEN 'manager' THEN 'manager' ELSE 'mod' END) END,a.role,a.flags,a.allow_boards,a.deny_boards
      INTO label,v_role,v_flags,v_allow,v_deny FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
      WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id
        AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
        AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin') AND (a.role<>'janitor' OR v_board='j') AND staff_identity.has_board_access(a.id,v_board) FOR SHARE OF a,s;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF v_board<>'j' THEN
        options:=coalesce(intent.source_options,CASE intent.capcode
            WHEN 'mod' THEN 'capcode_mod' WHEN 'developer' THEN 'capcode_dev'
            WHEN 'manager' THEN 'capcode_manager' WHEN 'admin' THEN 'capcode_admin'
            WHEN 'founder' THEN 'capcode_founder' WHEN 'admin_highlight' THEN 'capcode_admin_hl' END);
        chosen:=staff_identity.source_public_capcode(v_role,v_flags,v_allow,v_deny,options);
        named:=staff_identity.source_capcode_name_allowed(v_role,v_flags,v_allow,v_deny);
        IF chosen IS DISTINCT FROM intent.capcode
            OR (NOT named AND (intent.name<>'Anonymous' OR intent.prepared_trip IS NOT NULL))
            OR (intent.source_options IS NOT NULL AND intent.source_name_allowed IS DISTINCT FROM named)
            OR (v_forced AND v_role<>'admin' AND (intent.name<>'Anonymous' OR intent.prepared_trip IS NOT NULL))
            OR (v_forced AND intent.subject<>'') OR (v_strip AND intent.prepared_trip IS NOT NULL)
            OR content.staff_display_name_size(intent.name,intent.prepared_trip)>255 THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
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
       OR (intent.source_options IS NULL AND label IS DISTINCT FROM (CASE intent.capcode WHEN 'admin_highlight' THEN 'admin' ELSE intent.capcode END)) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    PERFORM set_config('board.staff_authorized_limits',CASE WHEN intent.authorized_limits THEN 'true' ELSE 'false' END,true);
    PERFORM set_config('board.post_trip',coalesce(intent.prepared_trip,''),true),
        set_config('board.staff_is_admin',CASE WHEN v_role='admin' AND v_board<>'j' THEN 'true' ELSE 'false' END,true);
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
    PERFORM set_config('board.staff_is_admin','false',true);
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

CREATE OR REPLACE FUNCTION content.apply_forced_anonymous() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_forced boolean;
BEGIN
    SELECT forced_anon INTO v_forced FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    IF v_forced THEN
        IF NOT (current_user='board_staff' AND coalesce(current_setting('board.staff_is_admin',true),'false')='true') THEN
            NEW.name:='Anonymous';
        END IF;
        NEW.subject:='';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION content.apply_post_trip() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_suppressed boolean;
BEGIN
    SELECT strip_tripcode OR (forced_anon AND NOT (
        current_user='board_staff' AND coalesce(current_setting('board.staff_is_admin',true),'false')='true'))
        INTO v_suppressed FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    IF v_suppressed AND NEW.name='' THEN NEW.name:='Anonymous'; END IF;
    NEW.trip:=CASE WHEN v_suppressed THEN NULL ELSE nullif(current_setting('board.post_trip',true),'') END;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_staff_capcode(),content.apply_forced_anonymous(),content.apply_post_trip() FROM PUBLIC;
