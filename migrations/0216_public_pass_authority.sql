-- Private membership state. Public, staff, mail and authentication logins receive
-- no table access; only the isolated verifier calls the finite issuer functions.
GRANT CREATE ON SCHEMA verification_private,content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE TABLE verification_private.pass_policy (
 singleton boolean PRIMARY KEY CHECK(singleton),
 entitlement_limit integer NOT NULL DEFAULT 50000 CHECK(entitlement_limit BETWEEN 1 AND 1000000),
 session_limit integer NOT NULL DEFAULT 100000 CHECK(session_limit BETWEEN 1 AND 1000000),
 attempt_limit integer NOT NULL DEFAULT 50000 CHECK(attempt_limit BETWEEN 1 AND 1000000)
);
INSERT INTO verification_private.pass_policy(singleton) VALUES(true);
CREATE TABLE verification_private.pass_entitlements (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 token_hash bytea NOT NULL UNIQUE CHECK(octet_length(token_hash)=32),
 pin_hash text NOT NULL CHECK(octet_length(pin_hash) BETWEEN 40 AND 512),
 status smallint NOT NULL CHECK(status BETWEEN 0 AND 7),
 purchased_at timestamptz NOT NULL CHECK(isfinite(purchased_at)),
 expires_at timestamptz CHECK(expires_at IS NULL OR isfinite(expires_at)),
 credential_version bigint NOT NULL DEFAULT 1 CHECK(credential_version>0),
 last_peer bytea CHECK(last_peer IS NULL OR octet_length(last_peer)=32),
 login_scope bytea CHECK(login_scope IS NULL OR octet_length(login_scope)=32),
 posting_scope bytea CHECK(posting_scope IS NULL OR octet_length(posting_scope)=32),
 last_used timestamptz CHECK(last_used IS NULL OR isfinite(last_used)),
 CHECK(status=7 OR expires_at IS NOT NULL),
 CHECK((last_peer IS NULL AND login_scope IS NULL AND posting_scope IS NULL AND last_used IS NULL)
  OR (last_peer IS NOT NULL AND last_used IS NOT NULL))
);
CREATE TABLE verification_private.pass_sessions (
 token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
 entitlement_id bigint NOT NULL REFERENCES verification_private.pass_entitlements(id) ON DELETE CASCADE,
 credential_version bigint NOT NULL CHECK(credential_version>0),
 created_at timestamptz NOT NULL CHECK(isfinite(created_at)),
 expires_at timestamptz NOT NULL CHECK(isfinite(expires_at) AND expires_at>created_at)
);
CREATE INDEX pass_sessions_entitlement ON verification_private.pass_sessions(entitlement_id,created_at);
CREATE INDEX pass_sessions_expiry ON verification_private.pass_sessions(expires_at);
CREATE TABLE verification_private.pass_attempts (
 ticket bytea PRIMARY KEY CHECK(octet_length(ticket)=32),
 peer bytea NOT NULL CHECK(octet_length(peer)=32),
 entitlement_id bigint REFERENCES verification_private.pass_entitlements(id) ON DELETE CASCADE,
 credential_version bigint,
 created_at timestamptz NOT NULL CHECK(isfinite(created_at)),
 finished boolean NOT NULL DEFAULT false,
 CHECK((entitlement_id IS NULL)=(credential_version IS NULL))
);
CREATE INDEX pass_attempts_peer ON verification_private.pass_attempts(peer,created_at);
CREATE INDEX pass_attempts_expiry ON verification_private.pass_attempts(created_at);
REVOKE ALL ON verification_private.pass_policy,verification_private.pass_entitlements,
 verification_private.pass_sessions,verification_private.pass_attempts FROM PUBLIC;

CREATE FUNCTION verification_private.pass_version() RETURNS trigger LANGUAGE plpgsql
SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.id<>OLD.id OR NEW.token_hash<>OLD.token_hash OR NEW.purchased_at<>OLD.purchased_at THEN
   RAISE EXCEPTION 'Membership identity is immutable' USING ERRCODE='23514'; END IF;
  IF NEW.pin_hash<>OLD.pin_hash OR NEW.status<>OLD.status THEN
   NEW.credential_version:=OLD.credential_version+1;
  ELSIF NEW.credential_version<>OLD.credential_version THEN
   RAISE EXCEPTION 'Membership version is owned by its lifecycle' USING ERRCODE='23514'; END IF;
 ELSE
  PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
  IF NOT FOUND OR (SELECT count(*) FROM verification_private.pass_entitlements)>=
    (SELECT entitlement_limit FROM verification_private.pass_policy WHERE singleton) THEN
   RAISE EXCEPTION 'Membership capacity reached' USING ERRCODE='54000'; END IF;
 END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_version() FROM PUBLIC;
CREATE TRIGGER pass_version BEFORE INSERT OR UPDATE ON verification_private.pass_entitlements
FOR EACH ROW EXECUTE FUNCTION verification_private.pass_version();

CREATE FUNCTION content.pass_begin_login(p_token bytea,p_peer bytea,p_ticket bytea)
RETURNS TABLE(status text,pin_hash text) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE target verification_private.pass_entitlements%ROWTYPE; stamp timestamptz:=date_trunc('second',clock_timestamp()); limits verification_private.pass_policy%ROWTYPE;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN
  RAISE EXCEPTION 'Membership issuer is unavailable' USING ERRCODE='42501'; END IF;
 PERFORM verification_private.pass_gate();
 IF p_token IS NULL OR octet_length(p_token)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
 OR p_ticket IS NULL OR octet_length(p_ticket)<>32 THEN RAISE EXCEPTION 'Invalid login context' USING ERRCODE='23514'; END IF;
 SELECT * INTO limits FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Membership policy unavailable' USING ERRCODE='55000'; END IF;
 DELETE FROM verification_private.pass_attempts WHERE ticket IN(SELECT ticket FROM verification_private.pass_attempts
  WHERE created_at<stamp-interval '1 hour' ORDER BY created_at LIMIT 64);
 IF (SELECT count(*) FROM verification_private.pass_attempts a WHERE a.peer=p_peer AND a.created_at>=stamp-interval '1 hour')>=5 THEN
  RETURN QUERY SELECT 'flood'::text,NULL::text; RETURN; END IF;
 IF (SELECT count(*) FROM verification_private.pass_attempts)>=limits.attempt_limit THEN
  RETURN QUERY SELECT 'capacity'::text,NULL::text; RETURN; END IF;
 SELECT * INTO target FROM verification_private.pass_entitlements WHERE token_hash=p_token;
 INSERT INTO verification_private.pass_attempts(ticket,peer,entitlement_id,credential_version,created_at)
 VALUES(p_ticket,p_peer,target.id,target.credential_version,stamp);
 RETURN QUERY SELECT 'verify'::text,target.pin_hash;
END $$;

CREATE FUNCTION content.pass_finish_login(p_ticket bytea,p_peer bytea,p_login_scope bytea,p_posting_scope bytea,
 p_verified boolean,p_session bytea,p_remember boolean)
RETURNS TABLE(status text,remaining integer,ttl_seconds integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE attempt verification_private.pass_attempts%ROWTYPE; target verification_private.pass_entitlements%ROWTYPE;
 stamp timestamptz; limits verification_private.pass_policy%ROWTYPE; ttl integer;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN
  RAISE EXCEPTION 'Membership issuer is unavailable' USING ERRCODE='42501'; END IF;
 PERFORM verification_private.pass_gate();
 IF p_ticket IS NULL OR octet_length(p_ticket)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
 OR (p_login_scope IS NOT NULL AND octet_length(p_login_scope)<>32)
 OR (p_posting_scope IS NOT NULL AND octet_length(p_posting_scope)<>32)
 OR p_verified IS NULL OR p_session IS NULL OR octet_length(p_session)<>32 OR p_remember IS NULL THEN
  RAISE EXCEPTION 'Invalid login completion' USING ERRCODE='23514'; END IF;
 SELECT * INTO limits FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Membership policy unavailable' USING ERRCODE='55000'; END IF;
 SELECT * INTO attempt FROM verification_private.pass_attempts WHERE ticket=p_ticket FOR UPDATE;
 stamp:=date_trunc('second',clock_timestamp());
 IF NOT FOUND OR attempt.peer<>p_peer OR attempt.finished OR attempt.created_at<stamp-interval '30 seconds' THEN
  RETURN QUERY SELECT 'rejected'::text,0,0; RETURN; END IF;
 UPDATE verification_private.pass_attempts SET finished=true WHERE ticket=p_ticket;
 SELECT * INTO target FROM verification_private.pass_entitlements WHERE id=attempt.entitlement_id FOR UPDATE;
 stamp:=date_trunc('second',clock_timestamp());
 IF NOT p_verified OR NOT FOUND OR target.credential_version<>attempt.credential_version
 OR attempt.created_at<stamp-interval '30 seconds' THEN
  RETURN QUERY SELECT 'rejected'::text,0,0; RETURN; END IF;
 -- Correct credentials do not consume the failed-attempt budget.
 DELETE FROM verification_private.pass_attempts WHERE ticket=p_ticket;
 IF target.login_scope IS NOT NULL AND target.login_scope IS DISTINCT FROM p_login_scope
 AND target.last_used>stamp-interval '900 seconds' THEN
  RETURN QUERY SELECT 'in_use'::text,ceil(extract(epoch FROM target.last_used+interval '900 seconds'-stamp))::integer,0; RETURN; END IF;
 IF target.status BETWEEN 1 AND 5 THEN
  RETURN QUERY SELECT ('disabled_'||target.status)::text,0,0; RETURN; END IF;
 DELETE FROM verification_private.pass_sessions WHERE token_hash IN(SELECT token_hash FROM verification_private.pass_sessions
  WHERE expires_at<=stamp ORDER BY expires_at LIMIT 64);
 IF (SELECT count(*) FROM verification_private.pass_sessions)>=limits.session_limit
 OR (SELECT count(*) FROM verification_private.pass_sessions WHERE entitlement_id=target.id)>=8 THEN
  RETURN QUERY SELECT 'capacity'::text,0,0; RETURN; END IF;
 UPDATE verification_private.pass_entitlements SET status=0,
  expires_at=CASE WHEN target.status=7 THEN stamp+interval '1 year' ELSE expires_at END,
  last_used=CASE WHEN last_peer IS DISTINCT FROM p_peer THEN stamp ELSE last_used END,
  last_peer=p_peer,login_scope=p_login_scope,posting_scope=p_posting_scope
 WHERE id=target.id RETURNING * INTO target;
 ttl:=CASE WHEN p_remember THEN 31556900 ELSE 86400 END;
 INSERT INTO verification_private.pass_sessions(token_hash,entitlement_id,credential_version,created_at,expires_at)
 VALUES(p_session,target.id,target.credential_version,stamp,stamp+make_interval(secs=>ttl));
 RETURN QUERY SELECT 'ready'::text,0,ttl;
END $$;

CREATE FUNCTION content.pass_signout(p_session bytea) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN
  RAISE EXCEPTION 'Membership issuer is unavailable' USING ERRCODE='42501'; END IF;
 PERFORM verification_private.pass_gate();
 IF p_session IS NULL OR octet_length(p_session)<>32 THEN RAISE EXCEPTION 'Invalid membership session' USING ERRCODE='23514'; END IF;
 PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 DELETE FROM verification_private.pass_sessions WHERE token_hash=p_session;
END $$;
CREATE FUNCTION content.pass_session_state(p_session bytea)
RETURNS TABLE(status text,purchase_year integer) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE saved verification_private.pass_sessions%ROWTYPE; target verification_private.pass_entitlements%ROWTYPE; stamp timestamptz:=date_trunc('second',clock_timestamp());
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN
  RAISE EXCEPTION 'Membership issuer is unavailable' USING ERRCODE='42501'; END IF;
 PERFORM verification_private.pass_gate();
 IF p_session IS NULL OR octet_length(p_session)<>32 THEN RAISE EXCEPTION 'Invalid membership session' USING ERRCODE='23514'; END IF;
 SELECT * INTO saved FROM verification_private.pass_sessions WHERE token_hash=p_session;
 IF NOT FOUND OR saved.expires_at<=stamp THEN RETURN QUERY SELECT 'none'::text,NULL::integer; RETURN; END IF;
 SELECT * INTO target FROM verification_private.pass_entitlements WHERE id=saved.entitlement_id;
 IF NOT FOUND OR target.credential_version<>saved.credential_version THEN RETURN QUERY SELECT 'none'::text,NULL::integer; RETURN; END IF;
 IF target.expires_at<=stamp THEN RETURN QUERY SELECT 'expired'::text,NULL::integer; RETURN; END IF;
 IF target.status<>0 THEN RETURN QUERY SELECT 'disabled'::text,NULL::integer; RETURN; END IF;
 RETURN QUERY SELECT 'ready'::text,extract(year FROM target.purchased_at AT TIME ZONE 'America/New_York')::integer;
END $$;
REVOKE ALL ON FUNCTION content.pass_session_state(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_session_state(bytea) TO board_verifier;

REVOKE ALL ON FUNCTION content.pass_begin_login(bytea,bytea,bytea),
 content.pass_finish_login(bytea,bytea,bytea,bytea,boolean,bytea,boolean),content.pass_signout(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_begin_login(bytea,bytea,bytea),
 content.pass_finish_login(bytea,bytea,bytea,bytea,boolean,bytea,boolean),content.pass_signout(bytea) TO board_verifier;
CREATE TABLE verification_private.pass_catalog (
 singleton boolean PRIMARY KEY CHECK(singleton),
 definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
REVOKE ALL ON verification_private.pass_catalog FROM PUBLIC;
CREATE FUNCTION verification_private.pass_acl(p_acl aclitem[]) RETURNS jsonb LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce(jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  ORDER BY pg_get_userbyid(a.grantor),a.grantee=0,pg_get_userbyid(a.grantee),a.privilege_type,a.is_grantable),'[]')
 FROM aclexplode(p_acl) a
$$;
CREATE FUNCTION verification_private.pass_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='verification_private' AND c.relname LIKE 'pass\_%' ESCAPE '\'
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname IN('verification_private','content') AND p.proname LIKE 'pass\_%' ESCAPE '\')
   OR (n.nspname='content' AND p.proname='public_pass_ready')
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
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),verification_private.pass_acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
    FROM pg_namespace n WHERE n.nspname='verification_private'))::text,'UTF8'))
$$;
CREATE FUNCTION content.public_pass_ready() RETURNS boolean LANGUAGE sql SECURITY DEFINER
SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT definition_hash=verification_private.pass_catalog_hash()
 FROM verification_private.pass_catalog WHERE singleton),false)
 AND EXISTS(SELECT 1 FROM verification_private.pass_policy WHERE singleton)
 AND NOT EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_verifier','board_mailer']) r(name)
  WHERE pg_has_role(r.name,'board_anonymous_owner','MEMBER')
   OR has_schema_privilege(r.name,'verification_private','USAGE')
   OR EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='verification_private' AND c.relname LIKE 'pass\_%' ESCAPE '\'
     AND (has_table_privilege(r.name,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
       OR has_any_column_privilege(r.name,c.oid,'SELECT,INSERT,UPDATE')) AND c.relkind='r'))
$$;
CREATE FUNCTION verification_private.pass_gate() RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR SHARE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Membership authority unavailable' USING ERRCODE='55000'; END IF;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_acl(aclitem[]),verification_private.pass_catalog_hash(),
 verification_private.pass_gate(),content.public_pass_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.public_pass_ready() TO board_verifier,board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private,content FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_catalog VALUES(true,verification_private.pass_catalog_hash());
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Initial membership authority failed'; END IF; END $$;
RESET ROLE;
