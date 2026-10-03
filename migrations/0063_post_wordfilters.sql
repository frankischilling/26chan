-- Policies apply to future posts only. Historical comments remain unchanged.
ALTER TABLE content.boards
    ADD COLUMN word_filter_enabled boolean NOT NULL DEFAULT false,
    ADD COLUMN word_filter_profile smallint NOT NULL DEFAULT 0
        CHECK (word_filter_profile BETWEEN 0 AND 4);
ALTER TABLE content.posts ADD COLUMN wordfilter_payload bytea
    CHECK (wordfilter_payload IS NULL OR (octet_length(wordfilter_payload) BETWEEN 11 AND 131072
        AND substring(wordfilter_payload FROM 1 FOR 4)=decode('57463031','hex')));
ALTER TABLE content.posts ADD COLUMN wordfilter_search text;
ALTER TABLE content.posts ADD CONSTRAINT posts_wordfilter_search_check CHECK (
    (wordfilter_payload IS NULL AND wordfilter_search IS NULL)
    OR (wordfilter_payload IS NOT NULL AND wordfilter_search IS NOT NULL AND octet_length(wordfilter_search)<=131072)
);
ALTER TABLE content.posts DROP CONSTRAINT posts_comment_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_check CHECK (
    (wordfilter_payload IS NULL AND char_length(comment) BETWEEN 0 AND 16000 AND octet_length(comment)<=64000)
    OR (wordfilter_payload IS NOT NULL AND octet_length(comment)<=524288)
);
GRANT SELECT(word_filter_enabled,word_filter_profile) ON content.boards TO board_attachment_owner;

-- The runtime supplies bounded typed data under the same board lock. Text in
-- this encoding still has no HTML authority; the Rust decoder validates every
-- discriminator, length, Unicode string, tag and random choice before rendering.
CREATE FUNCTION content.stamp_wordfilter_payload() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean; payload text;
BEGIN
    SELECT b.word_filter_enabled INTO enabled FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    NEW.wordfilter_payload:=NULL;
    NEW.wordfilter_search:=NULL;
    IF enabled THEN
        payload:=current_setting('board.wordfilter_payload',true);
        IF payload IS NULL OR length(payload) NOT BETWEEN 22 AND 262144 OR payload !~ '^[0-9a-f]+$'
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
-- Staff authority is checked first, preserving its denial contract. This
-- stamp and its constraints still run before the insertion can commit.
CREATE TRIGGER b_stamp_wordfilter_payload BEFORE INSERT ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.stamp_wordfilter_payload();

ALTER TABLE post_secrets.staff_post_intents ADD COLUMN wordfilter_payload bytea
    CHECK (wordfilter_payload IS NULL OR (octet_length(wordfilter_payload) BETWEEN 11 AND 131072
        AND substring(wordfilter_payload FROM 1 FOR 4)=decode('57463031','hex')));
ALTER TABLE post_secrets.staff_post_intents ADD COLUMN wordfilter_search text
    CHECK (wordfilter_search IS NULL OR octet_length(wordfilter_search)<=131072);
ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_comment_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_comment_check
    CHECK (octet_length(comment)<=524288);
GRANT UPDATE(wordfilter_payload,wordfilter_search) ON post_secrets.staff_post_intents TO board_staff_post_owner;
GRANT CREATE ON SCHEMA staff_identity,content TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;

-- Existing session, rank, scope, revocation, capacity and /j/ checks are owned
-- by issue_post_authority. Bind the exact typed body to that same ticket.
CREATE FUNCTION staff_identity.issue_wordfiltered_post_authority(
    ticket bytea,session_token bytea,csrf bytea,idle integer,highlight boolean,
    post_number bigint,v_board text,v_thread bigint,v_name text,v_subject text,v_comment text,v_time timestamptz,
    v_wordfilter_payload bytea,v_wordfilter_search text
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF v_wordfilter_payload IS NOT NULL AND (octet_length(v_wordfilter_payload) NOT BETWEEN 11 AND 131072
        OR substring(v_wordfilter_payload FROM 1 FOR 4)<>decode('57463031','hex')) THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF (v_wordfilter_payload IS NULL)<>(v_wordfilter_search IS NULL)
        OR octet_length(v_wordfilter_search)>131072 THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    PERFORM staff_identity.issue_post_authority(ticket,session_token,csrf,idle,highlight,
        post_number,v_board,v_thread,v_name,v_subject,v_comment,v_time);
    UPDATE post_secrets.staff_post_intents SET wordfilter_payload=v_wordfilter_payload,wordfilter_search=v_wordfilter_search WHERE token_hash=ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
END $$;
REVOKE ALL ON FUNCTION staff_identity.issue_wordfiltered_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.issue_wordfiltered_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,bytea,text) TO board_auth;

-- The current consume function, with exact saved-body binding, follows below.

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
        OR intent.posted_at IS DISTINCT FROM v_time
        OR intent.wordfilter_payload IS DISTINCT FROM decode(nullif(current_setting('board.wordfilter_payload',true),''),'hex')
        OR intent.wordfilter_search IS DISTINCT FROM (CASE WHEN intent.wordfilter_payload IS NOT NULL THEN current_setting('board.wordfilter_search',true) ELSE NULL END) THEN
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
REVOKE CREATE ON SCHEMA staff_identity,content FROM board_staff_post_owner;

-- Pinned source WORD_FILT switches and board-file replacement of the global filter.
UPDATE content.boards b SET word_filter_enabled=policy.enabled,word_filter_profile=policy.profile
FROM (VALUES
('a',true,0),
('aco',true,0),
('c',true,0),
('d',true,0),
('e',true,0),
('f',true,0),
('g',true,0),
('gif',true,0),
('h',true,0),
('his',true,0),
('hr',true,0),
('k',true,0),
('m',true,0),
('n',true,0),
('o',true,0),
('p',true,0),
('r',true,0),
('s',true,0),
('t',true,0),
('u',true,0),
('v',true,3),
('w',true,0),
('wg',true,0),
('i',true,0),
('ic',true,0),
('cm',true,0),
('y',true,0),
('an',true,0),
('cgl',true,0),
('ck',true,1),
('co',true,0),
('fa',true,0),
('fit',true,0),
('jp',true,0),
('mlp',true,0),
('mu',true,0),
('po',true,0),
('sp',true,0),
('tg',true,0),
('toy',true,0),
('trv',true,0),
('tv',true,0),
('x',true,0),
('b',false,0),
('soc',true,0),
('r9k',true,0),
('test',true,4),
('adv',true,0),
('lit',true,0),
('int',true,1),
('sci',true,0),
('3',true,0),
('vp',true,0),
('diy',true,0),
('pol',true,0),
('hc',true,0),
('vg',true,0),
('hm',true,0),
('j',true,0),
('wsg',true,0),
('out',true,0),
('lgbt',true,0),
('vr',true,0),
('gd',true,0),
('s4s',false,0),
('biz',true,0),
('qa',true,0),
('trash',true,0),
('news',false,0),
('wsr',true,0),
('qst',true,0),
('bant',true,0),
('vip',true,0),
('vrpg',true,0),
('vmg',true,0),
('vst',true,0),
('vt',true,0),
('vm',true,0),
('pw',true,0),
('xs',true,0),
('asp',true,2),
('qb',true,0)) policy(slug,enabled,profile) WHERE b.slug=policy.slug;
