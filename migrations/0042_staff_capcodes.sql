ALTER TABLE content.posts ADD COLUMN capcode text
    CHECK (capcode IS NULL OR capcode IN ('mod','admin','admin_highlight','manager','developer','founder'));
ALTER TABLE staff_identity.accounts ADD COLUMN public_capcode text
    CHECK (public_capcode IS NULL OR (role='moderator' AND public_capcode='mod')
        OR (role='admin' AND public_capcode IN ('mod','admin','manager','developer','founder')));

CREATE TABLE post_secrets.staff_post_intents (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash)=32),
    session_hash bytea NOT NULL REFERENCES staff_identity.sessions(token_hash) ON DELETE CASCADE,
    account_id bigint NOT NULL REFERENCES staff_identity.accounts(id),
    capcode text NOT NULL CHECK (capcode IN ('mod','admin','admin_highlight','manager','developer','founder')),
    post_id bigint NOT NULL CHECK (post_id>0),
    board text NOT NULL CHECK (board ~ '^[a-z0-9]{1,10}$'),
    thread_id bigint NOT NULL CHECK (thread_id>0 AND thread_id<=post_id),
    name text NOT NULL CHECK (octet_length(name) BETWEEN 1 AND 100),
    subject text NOT NULL CHECK (octet_length(subject)<=600),
    comment text NOT NULL CHECK (octet_length(comment)<=64000),
    posted_at timestamptz NOT NULL,
    idle_seconds integer NOT NULL CHECK (idle_seconds BETWEEN 60 AND 3600),
    expires_at timestamptz NOT NULL DEFAULT clock_timestamp()+interval '15 seconds'
);
CREATE INDEX staff_post_intents_expiration ON post_secrets.staff_post_intents(expires_at);
CREATE INDEX staff_post_intents_account ON post_secrets.staff_post_intents(account_id);
REVOKE ALL ON post_secrets.staff_post_intents FROM PUBLIC;
GRANT USAGE ON SCHEMA content,post_secrets,staff_identity TO board_staff_post_owner;
GRANT SELECT,INSERT,DELETE ON post_secrets.staff_post_intents TO board_staff_post_owner;
-- PostgreSQL requires UPDATE authority for the single-use row lock.
GRANT UPDATE(token_hash) ON post_secrets.staff_post_intents TO board_staff_post_owner;
GRANT SELECT(token_hash,csrf_hash,account_id,credential_id,authenticated_at,expires_at,last_activity_at),
    UPDATE(token_hash) ON staff_identity.sessions TO board_staff_post_owner;
GRANT SELECT(id,role,revoked_at,public_capcode),UPDATE(id) ON staff_identity.accounts TO board_staff_post_owner;
GRANT INSERT(account_id,board,target_id,action) ON content.moderation_audit TO board_staff_post_owner;
GRANT USAGE ON SEQUENCE content.moderation_audit_id_seq TO board_staff_post_owner;

CREATE FUNCTION staff_identity.issue_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,highlight boolean,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE actor bigint; label text;
BEGIN
    IF octet_length(ticket)<>32 OR octet_length(session_token)<>32 OR octet_length(csrf)<>32
       OR idle NOT BETWEEN 60 AND 3600 OR highlight IS NULL THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    SELECT a.id,coalesce(a.public_capcode,CASE a.role WHEN 'admin' THEN 'admin' ELSE 'mod' END)
      INTO actor,label FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
      WHERE s.token_hash=session_token AND s.csrf_hash=csrf AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>idle)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
        AND a.revoked_at IS NULL AND a.role IN ('moderator','admin') FOR UPDATE OF a FOR SHARE OF s;
    IF NOT FOUND OR (highlight AND label<>'admin') OR NOT EXISTS (
        SELECT 1 FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
        WHERE s.token_hash=session_token AND s.csrf_hash=csrf AND s.expires_at>clock_timestamp()
          AND s.last_activity_at>clock_timestamp()-make_interval(secs=>idle)
          AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
          AND a.revoked_at IS NULL AND a.role IN ('moderator','admin')) THEN
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

CREATE FUNCTION content.consume_staff_post_authority(ticket bytea,post_number bigint,v_board text,
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
    SELECT coalesce(a.public_capcode,CASE a.role WHEN 'admin' THEN 'admin' ELSE 'mod' END)
      INTO label FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id
      WHERE s.token_hash=intent.session_hash AND a.id=intent.account_id
        AND s.expires_at>clock_timestamp()
        AND s.last_activity_at>clock_timestamp()-make_interval(secs=>intent.idle_seconds)
        AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
        AND a.revoked_at IS NULL AND a.role IN ('moderator','admin') FOR SHARE OF a,s;
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
          AND a.revoked_at IS NULL AND a.role IN ('moderator','admin'))
       OR label IS DISTINCT FROM (CASE intent.capcode WHEN 'admin_highlight' THEN 'admin' ELSE intent.capcode END) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    DELETE FROM post_secrets.staff_post_intents WHERE token_hash=ticket;
    INSERT INTO content.moderation_audit(account_id,board,target_id,action)
      VALUES(intent.account_id,v_board,post_number,'staff-post');
    RETURN intent.capcode;
END $$;

CREATE FUNCTION content.apply_staff_capcode() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE ticket text;
BEGIN
    NEW.capcode:=NULL;
    IF current_user='board_staff' THEN
        ticket:=current_setting('board.staff_post_ticket',true);
        IF ticket IS NULL OR ticket !~ '^[0-9a-f]{64}$' THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
        NEW.capcode:=content.consume_staff_post_authority(decode(ticket,'hex'),NEW.id,NEW.board,
            NEW.thread_id,NEW.name,NEW.subject,NEW.comment,NEW.created_at);
        PERFORM set_config('board.poster_fingerprint','',true),set_config('board.poster_epoch','',true);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER a_apply_staff_capcode BEFORE INSERT ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.apply_staff_capcode();

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK (action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post'));
GRANT INSERT(id,board,created_at,modified_at) ON content.threads TO board_staff;
GRANT UPDATE(reply_count,bumped_at,archived_at,archive_expires_at) ON content.threads TO board_staff;
GRANT INSERT(id,board,thread_id,name,subject,comment,created_at) ON content.posts TO board_staff;
GRANT USAGE ON SEQUENCE content.post_number TO board_staff;
GRANT SELECT ON content.visible_threads TO board_staff;

REVOKE ALL ON FUNCTION staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz),
    content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz),content.apply_staff_capcode() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz) TO board_auth;
GRANT EXECUTE ON FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz) TO board_staff;
GRANT CREATE ON SCHEMA content,staff_identity TO board_staff_post_owner;
ALTER FUNCTION staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz) OWNER TO board_staff_post_owner;
ALTER FUNCTION content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz) OWNER TO board_staff_post_owner;
REVOKE CREATE ON SCHEMA content,staff_identity FROM board_staff_post_owner;

CREATE OR REPLACE FUNCTION content.apply_post_trip() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE v_forced boolean;
BEGIN
    IF NEW.capcode IS NOT NULL THEN NEW.trip:=NULL; RETURN NEW; END IF;
    SELECT forced_anon INTO v_forced FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    NEW.trip := CASE WHEN v_forced THEN NULL
        ELSE nullif(current_setting('board.post_trip', true), '') END;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION content.apply_poster_id() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE v_enabled boolean;
BEGIN
    IF NEW.capcode IS NOT NULL THEN NEW.poster_id:=NULL; RETURN NEW; END IF;
    SELECT user_ids INTO v_enabled FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    NEW.poster_id := CASE WHEN v_enabled THEN nullif(current_setting('board.poster_id', true), '') ELSE NULL END;
    IF v_enabled AND NEW.poster_id IS NULL THEN
        RAISE EXCEPTION 'Poster identity is unavailable.' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION content.apply_post_flag() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_geo boolean; v_flags text[]; v_selected text;
BEGIN
    IF NEW.capcode IS NOT NULL THEN NEW.country:=NULL; NEW.country_name:=NULL; NEW.board_flag:=NULL; NEW.flag_name:=NULL; RETURN NEW; END IF;
    SELECT country_flags,board_flags INTO v_geo,v_flags FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    v_selected := coalesce(nullif(current_setting('board.flag',true),''),'0');
    NEW.country := NULL; NEW.country_name := NULL; NEW.board_flag := NULL; NEW.flag_name := NULL;
    IF v_selected <> '0' THEN
        IF NOT v_selected=ANY(v_flags) THEN
            RAISE EXCEPTION 'Invalid board flag.' USING ERRCODE='23514';
        END IF;
        NEW.board_flag := v_selected;
        NEW.flag_name := (ARRAY['Anarcho-Capitalist','Anarchist','Black Nationalist','Confederate','Communist','Catalonia','Democrat','European','Fascist','Gadsden','Gay','Jihadi','Kekistani','Muslim','National Bolshevik','NATO','Nazi','Hippie','Pirate','Republican','Task Force Z','Templar','Tree Hugger','United Nations','White Supremacist']::text[])[array_position(ARRAY['AC','AN','BL','CF','CM','CT','DM','EU','FC','GN','GY','JH','KN','MF','NB','NT','NZ','PC','PR','RE','MZ','TM','TR','UN','WP']::text[],v_selected)];
    ELSIF v_geo THEN
        NEW.country := nullif(current_setting('board.country',true),'');
        NEW.country_name := nullif(current_setting('board.country_name',true),'');
        IF NEW.country IS NULL OR NEW.country_name IS NULL THEN
            RAISE EXCEPTION 'Country flags are unavailable.' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END $$;
