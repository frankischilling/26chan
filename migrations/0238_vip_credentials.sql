-- Separate VIP credentials. A VIP name is not a staff identity or a Pass.
-- These functions issue bounded receipts; posting consumption is added next.
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
    PERFORM verification_private.pass_gate();
    IF content.public_challenge_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Challenge authority unavailable before VIP storage';
    END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA verification_private,content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;

CREATE TABLE verification_private.vip_policy (
    singleton boolean PRIMARY KEY CHECK(singleton),
    account_limit integer NOT NULL DEFAULT 10000 CHECK(account_limit BETWEEN 1 AND 100000),
    attempt_limit integer NOT NULL DEFAULT 50000 CHECK(attempt_limit BETWEEN 1 AND 1000000),
    receipt_limit integer NOT NULL DEFAULT 50000 CHECK(receipt_limit BETWEEN 1 AND 1000000)
);
INSERT INTO verification_private.vip_policy(singleton) VALUES(true);
CREATE TABLE verification_private.vip_accounts (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    identifier_hash bytea NOT NULL UNIQUE CHECK(octet_length(identifier_hash)=32),
    name text NOT NULL CHECK(octet_length(name) BETWEEN 1 AND 100 AND name !~ '[[:cntrl:]]'),
    password_hash text NOT NULL CHECK(octet_length(password_hash) BETWEEN 40 AND 256),
    active boolean NOT NULL DEFAULT true,
    credential_version bigint NOT NULL DEFAULT 1 CHECK(credential_version>0),
    last_peer bytea CHECK(last_peer IS NULL OR octet_length(last_peer)=32),
    last_used timestamptz CHECK(last_used IS NULL OR isfinite(last_used)),
    CHECK((last_peer IS NULL)=(last_used IS NULL))
);
CREATE TABLE verification_private.vip_attempts (
    ticket bytea PRIMARY KEY CHECK(octet_length(ticket)=32),
    peer bytea NOT NULL CHECK(octet_length(peer)=32),
    -- Retain failed attempts when an account is removed; no cascading FK.
    account_id bigint CHECK(account_id IS NULL OR account_id>0),
    credential_version bigint CHECK(credential_version IS NULL OR credential_version>0),
    anonymous_session bytea NOT NULL CHECK(octet_length(anonymous_session)=32),
    context_hash bytea NOT NULL CHECK(octet_length(context_hash)=32),
    board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$'),
    target bigint NOT NULL CHECK(target>=0),
    pass_receipt bytea CHECK(pass_receipt IS NULL OR octet_length(pass_receipt)=32),
    pass_context bytea CHECK(pass_context IS NULL OR octet_length(pass_context)=32),
    created_at timestamptz NOT NULL CHECK(isfinite(created_at)),
    finished boolean NOT NULL DEFAULT false,
    CHECK((account_id IS NULL)=(credential_version IS NULL)),
    CHECK((pass_receipt IS NULL)=(pass_context IS NULL))
);
CREATE INDEX vip_attempts_peer ON verification_private.vip_attempts(peer,created_at);
CREATE INDEX vip_attempts_expiry ON verification_private.vip_attempts(created_at);
CREATE TABLE verification_private.vip_receipts (
    token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
    account_id bigint NOT NULL REFERENCES verification_private.vip_accounts(id) ON DELETE CASCADE,
    credential_version bigint NOT NULL CHECK(credential_version>0),
    peer bytea NOT NULL CHECK(octet_length(peer)=32),
    anonymous_session bytea NOT NULL CHECK(octet_length(anonymous_session)=32),
    context_hash bytea NOT NULL CHECK(octet_length(context_hash)=32),
    board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$'),
    target bigint NOT NULL CHECK(target>=0),
    created_at timestamptz NOT NULL CHECK(isfinite(created_at)),
    expires_at timestamptz NOT NULL CHECK(isfinite(expires_at) AND expires_at>created_at)
);
CREATE INDEX vip_receipts_account ON verification_private.vip_receipts(account_id,expires_at);
CREATE INDEX vip_receipts_expiry ON verification_private.vip_receipts(expires_at);
CREATE TABLE verification_private.vip_catalog (
    singleton boolean PRIMARY KEY CHECK(singleton),
    definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
REVOKE ALL ON verification_private.vip_policy,verification_private.vip_accounts,
    verification_private.vip_attempts,verification_private.vip_receipts,
    verification_private.vip_catalog FROM PUBLIC;

CREATE FUNCTION verification_private.vip_version() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        PERFORM singleton FROM verification_private.vip_policy WHERE singleton FOR UPDATE;
        IF NOT FOUND OR (SELECT count(*) FROM verification_private.vip_accounts)>=
            (SELECT account_limit FROM verification_private.vip_policy WHERE singleton) THEN
            RAISE EXCEPTION 'VIP account capacity reached' USING ERRCODE='54000';
        END IF;
        NEW.credential_version:=1;
    ELSE
        IF NEW.id<>OLD.id OR NEW.identifier_hash<>OLD.identifier_hash THEN
            RAISE EXCEPTION 'VIP identity is immutable' USING ERRCODE='23514';
        END IF;
        IF NEW.password_hash<>OLD.password_hash OR NEW.active<>OLD.active OR NEW.name<>OLD.name THEN
            NEW.credential_version:=OLD.credential_version+1;
        ELSIF NEW.credential_version<>OLD.credential_version THEN
            RAISE EXCEPTION 'VIP version is owned by its lifecycle' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER vip_version BEFORE INSERT OR UPDATE ON verification_private.vip_accounts
FOR EACH ROW EXECUTE FUNCTION verification_private.vip_version();

-- Check the actual source bypass predicate. An ordinary solved CAPTCHA or a
-- courtesy-credit receipt does not qualify. This retains Pass locks, but does
-- not consume that independent posting receipt.
CREATE FUNCTION verification_private.vip_bypass(
    p_board text,p_target bigint,p_peer bytea,p_anonymous bytea,
    p_pass_receipt bytea,p_pass_context bytea
) RETURNS boolean LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean;
BEGIN
    SELECT b.captcha_enabled INTO enabled FROM content.boards b
        WHERE b.slug=p_board AND NOT b.staff_only;
    IF NOT FOUND THEN RETURN false; END IF;
    IF NOT enabled THEN RETURN true; END IF;
    IF p_pass_receipt IS NULL THEN RETURN false; END IF;
    BEGIN
        PERFORM verification_private.pass_checked_receipt(
            p_pass_receipt,p_peer,p_anonymous,p_pass_context,
            CASE WHEN p_target=0 THEN 'thread' ELSE 'reply' END,p_board,p_target);
    EXCEPTION WHEN SQLSTATE 'P0166' THEN
        RETURN false;
    END;
    RETURN true;
END $$;

CREATE FUNCTION content.vip_begin_login(
    p_identifier bytea,p_peer bytea,p_ticket bytea,p_anonymous bytea,p_context bytea,
    p_board text,p_target bigint,p_pass_receipt bytea,p_pass_context bytea
) RETURNS TABLE(status text,password_hash text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account verification_private.vip_accounts%ROWTYPE;
    limits verification_private.vip_policy%ROWTYPE; stamp timestamptz;
BEGIN
    IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN
        RAISE EXCEPTION 'VIP issuer unavailable' USING ERRCODE='42501';
    END IF;
    PERFORM verification_private.vip_gate();
    IF p_identifier IS NULL OR octet_length(p_identifier)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
        OR p_ticket IS NULL OR octet_length(p_ticket)<>32 OR p_anonymous IS NULL OR octet_length(p_anonymous)<>32
        OR p_context IS NULL OR octet_length(p_context)<>32 OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
        OR p_target IS NULL OR p_target<0 OR (p_pass_receipt IS NULL)<>(p_pass_context IS NULL)
        OR (p_pass_receipt IS NOT NULL AND octet_length(p_pass_receipt)<>32)
        OR (p_pass_context IS NOT NULL AND octet_length(p_pass_context)<>32) THEN
        RAISE EXCEPTION 'Invalid VIP login context' USING ERRCODE='23514';
    END IF;
    IF NOT verification_private.vip_bypass(p_board,p_target,p_peer,p_anonymous,p_pass_receipt,p_pass_context) THEN
        RETURN QUERY SELECT 'ineligible'::text,NULL::text; RETURN;
    END IF;
    SELECT * INTO STRICT limits FROM verification_private.vip_policy WHERE singleton FOR UPDATE;
    stamp:=date_trunc('second',clock_timestamp());
    DELETE FROM verification_private.vip_attempts WHERE ticket IN(
        SELECT a.ticket FROM verification_private.vip_attempts a
        WHERE a.created_at<stamp-interval '1 hour' ORDER BY a.created_at LIMIT 64);
    IF (SELECT count(*) FROM verification_private.vip_attempts a
        WHERE a.peer=p_peer AND a.created_at>=stamp-interval '1 hour')>=3 THEN
        RETURN QUERY SELECT 'flood'::text,NULL::text; RETURN;
    END IF;
    IF (SELECT count(*) FROM verification_private.vip_attempts)>=limits.attempt_limit THEN
        RETURN QUERY SELECT 'capacity'::text,NULL::text; RETURN;
    END IF;
    SELECT * INTO account FROM verification_private.vip_accounts v WHERE v.identifier_hash=p_identifier AND v.active;
    INSERT INTO verification_private.vip_attempts(ticket,peer,account_id,credential_version,
        anonymous_session,context_hash,board,target,pass_receipt,pass_context,created_at)
    VALUES(p_ticket,p_peer,account.id,account.credential_version,p_anonymous,p_context,
        p_board,p_target,p_pass_receipt,p_pass_context,stamp);
    RETURN QUERY SELECT 'verify'::text,account.password_hash;
END $$;

CREATE FUNCTION content.vip_finish_login(p_ticket bytea,p_peer bytea,p_verified boolean,p_receipt bytea)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE attempt verification_private.vip_attempts%ROWTYPE;
    account verification_private.vip_accounts%ROWTYPE;
    locked_attempt verification_private.vip_attempts%ROWTYPE;
    limits verification_private.vip_policy%ROWTYPE; stamp timestamptz;
BEGIN
    IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN
        RAISE EXCEPTION 'VIP issuer unavailable' USING ERRCODE='42501';
    END IF;
    PERFORM verification_private.vip_gate();
    IF p_ticket IS NULL OR octet_length(p_ticket)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
        OR p_verified IS NULL OR p_receipt IS NULL OR octet_length(p_receipt)<>32 THEN
        RAISE EXCEPTION 'Invalid VIP login completion' USING ERRCODE='23514';
    END IF;
    -- Read context before taking Pass locks; never reverse Pass -> VIP order.
    SELECT * INTO attempt FROM verification_private.vip_attempts WHERE ticket=p_ticket;
    IF NOT FOUND OR attempt.peer<>p_peer OR attempt.finished
        OR attempt.created_at<clock_timestamp()-interval '30 seconds' THEN RETURN 'rejected'; END IF;
    IF NOT verification_private.vip_bypass(attempt.board,attempt.target,p_peer,
        attempt.anonymous_session,attempt.pass_receipt,attempt.pass_context) THEN RETURN 'ineligible'; END IF;
    SELECT * INTO STRICT limits FROM verification_private.vip_policy WHERE singleton FOR UPDATE;
    SELECT * INTO locked_attempt FROM verification_private.vip_attempts WHERE ticket=p_ticket FOR UPDATE;
    IF NOT FOUND OR locked_attempt IS DISTINCT FROM attempt
        OR attempt.created_at<clock_timestamp()-interval '30 seconds' THEN RETURN 'rejected'; END IF;
    UPDATE verification_private.vip_attempts SET finished=true WHERE ticket=p_ticket;
    SELECT * INTO account FROM verification_private.vip_accounts WHERE id=attempt.account_id FOR UPDATE;
    stamp:=clock_timestamp();
    IF NOT p_verified OR NOT FOUND OR NOT account.active
        OR account.credential_version<>attempt.credential_version
        OR attempt.created_at<stamp-interval '30 seconds' THEN RETURN 'rejected'; END IF;
    DELETE FROM verification_private.vip_attempts WHERE ticket=p_ticket;
    DELETE FROM verification_private.vip_receipts WHERE token_hash IN(
        SELECT r.token_hash FROM verification_private.vip_receipts r
        WHERE r.expires_at<=stamp ORDER BY r.expires_at LIMIT 64);
    DELETE FROM verification_private.vip_receipts WHERE account_id=account.id
        AND credential_version<>account.credential_version;
    IF (SELECT count(*) FROM verification_private.vip_receipts)>=limits.receipt_limit
        OR (SELECT count(*) FROM verification_private.vip_receipts WHERE account_id=account.id)>=8 THEN
        RETURN 'capacity';
    END IF;
    INSERT INTO verification_private.vip_receipts(token_hash,account_id,credential_version,
        peer,anonymous_session,context_hash,board,target,created_at,expires_at)
    VALUES(p_receipt,account.id,account.credential_version,p_peer,attempt.anonymous_session,
        attempt.context_hash,attempt.board,attempt.target,stamp,stamp+interval '30 seconds');
    UPDATE verification_private.vip_accounts SET last_peer=p_peer,last_used=date_trunc('second',stamp)
        WHERE id=account.id;
    RETURN 'ready';
END $$;

CREATE FUNCTION content.vip_manage(p_identifier bytea,p_name text,p_hash text,p_active boolean,p_create boolean)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint;
BEGIN
    IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_migrator' THEN
        RAISE EXCEPTION 'VIP provisioning unavailable' USING ERRCODE='42501';
    END IF;
    PERFORM verification_private.vip_gate();
    IF p_identifier IS NULL OR octet_length(p_identifier)<>32 OR p_create IS NULL
        OR (p_create AND (p_name IS NULL OR p_hash IS NULL OR p_active IS NULL))
        OR (NOT p_create AND p_name IS NULL AND p_hash IS NULL AND p_active IS NULL)
        OR (p_name IS NOT NULL AND (octet_length(p_name) NOT BETWEEN 1 AND 100 OR p_name ~ '[[:cntrl:]]'))
        OR (p_hash IS NOT NULL AND (octet_length(p_hash)>256
            OR p_hash !~ '^\$argon2id\$v=19\$m=19456,t=2,p=1\$[A-Za-z0-9+/]{22}\$[A-Za-z0-9+/]{43}$')) THEN
        RAISE EXCEPTION 'Invalid VIP provisioning operation' USING ERRCODE='23514';
    END IF;
    PERFORM singleton FROM verification_private.vip_policy WHERE singleton FOR UPDATE;
    IF p_create THEN
        INSERT INTO verification_private.vip_accounts(identifier_hash,name,password_hash,active)
            VALUES(p_identifier,p_name,p_hash,p_active) RETURNING id INTO account;
    ELSE
        UPDATE verification_private.vip_accounts SET name=coalesce(p_name,name),
            password_hash=coalesce(p_hash,password_hash),active=coalesce(p_active,active)
            WHERE identifier_hash=p_identifier RETURNING id INTO account;
        IF NOT FOUND THEN RAISE EXCEPTION 'VIP identity not found' USING ERRCODE='P0002'; END IF;
    END IF;
    RETURN account;
END $$;

CREATE FUNCTION verification_private.vip_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
WITH relations AS (
    SELECT c.*,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='verification_private' AND c.relname LIKE 'vip\_%' ESCAPE '\'
), funcs AS (
    SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname IN('verification_private','content')
        AND (p.proname LIKE 'vip\_%' ESCAPE '\' OR p.proname='public_vip_ready')
)
SELECT sha256(convert_to(jsonb_build_array(
    (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
        r.relrowsecurity,r.relforcerowsecurity,r.reloptions,
        verification_private.pass_acl(coalesce(r.relacl,acldefault(CASE WHEN r.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,r.relowner))),
        CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
        (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
            pg_get_expr(d.adbin,d.adrelid),verification_private.pass_acl(a.attacl)) ORDER BY a.attnum)
         FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
         WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
        (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY k.conname)
         FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
    (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
        verification_private.pass_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
        ORDER BY p.oid::regprocedure::text) FROM funcs p),
    (SELECT jsonb_agg(jsonb_build_array(r.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY r.identity,t.tgname)
     FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal),
    (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
        t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
        ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
     FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
        SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
    (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),verification_private.pass_acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
     FROM pg_namespace n WHERE n.nspname='verification_private'))::text,'UTF8'))
$$;
CREATE FUNCTION content.public_vip_ready() RETURNS boolean LANGUAGE sql SECURITY DEFINER
SET search_path=pg_catalog,pg_temp AS $$
SELECT coalesce((SELECT definition_hash=verification_private.vip_catalog_hash()
    FROM verification_private.vip_catalog WHERE singleton),false)
    AND EXISTS(SELECT 1 FROM verification_private.vip_policy WHERE singleton)
    AND NOT EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_verifier','board_mailer',
        'board_media','board_media_read','board_media_intake','board_monitor','board_checkout']) r(name)
        WHERE pg_has_role(r.name,'board_anonymous_owner','MEMBER')
            OR has_schema_privilege(r.name,'verification_private','USAGE')
            OR EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
                WHERE n.nspname='verification_private' AND c.relname LIKE 'vip\_%' ESCAPE '\'
                    AND c.relkind='r' AND (has_table_privilege(r.name,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
                        OR has_any_column_privilege(r.name,c.oid,'SELECT,INSERT,UPDATE'))))
$$;
CREATE FUNCTION verification_private.vip_gate() RETURNS void LANGUAGE plpgsql
SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    PERFORM verification_private.pass_gate();
    PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR SHARE;
    IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'VIP authority unavailable' USING ERRCODE='55000';
    END IF;
END $$;
REVOKE ALL ON FUNCTION verification_private.vip_version(),
    verification_private.vip_bypass(text,bigint,bytea,bytea,bytea,bytea),
    content.vip_begin_login(bytea,bytea,bytea,bytea,bytea,text,bigint,bytea,bytea),
    content.vip_finish_login(bytea,bytea,boolean,bytea),
    content.vip_manage(bytea,text,text,boolean,boolean),verification_private.vip_catalog_hash(),
    content.public_vip_ready(),verification_private.vip_gate() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.vip_begin_login(bytea,bytea,bytea,bytea,bytea,text,bigint,bytea,bytea),
    content.vip_finish_login(bytea,bytea,boolean,bytea) TO board_verifier;
GRANT EXECUTE ON FUNCTION content.vip_manage(bytea,text,text,boolean,boolean) TO board_migrator;
GRANT EXECUTE ON FUNCTION content.public_vip_ready() TO board_public,board_verifier,board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private,content FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.vip_catalog VALUES(true,verification_private.vip_catalog_hash());
DO $$ BEGIN
    IF content.public_vip_ready() IS DISTINCT FROM true OR content.public_pass_ready() IS DISTINCT FROM true
        OR content.public_challenge_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'VIP storage changed existing authority';
    END IF;
END $$;
RESET ROLE;
