-- Source staff issuance and private identity binding. The posting decision is
-- independently compared with imgboard.php:8554; no cleartext UserPwd, Pass,
-- recovery password or transport address is stored here.
DO $restriction_preflight$
BEGIN
 IF (to_regclass('post_secrets.thread_transfer_catalog') IS NULL)
     <> (to_regprocedure('content.text_thread_transfer_ready()') IS NULL) THEN
  RAISE EXCEPTION 'Transfer catalog is incomplete before restrictions';
 END IF;
 IF to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
  EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
  PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
  IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
   RAISE EXCEPTION 'Transfer catalog is not ready before restrictions';
  END IF;
  EXECUTE 'RESET ROLE';
 END IF;
 IF (to_regclass('post_secrets.report_snapshot_catalog') IS NULL)
    <> (to_regprocedure('content.report_snapshots_ready()') IS NULL) THEN
  RAISE EXCEPTION 'Report capture catalog is incomplete before restrictions';
 END IF;
 IF to_regclass('post_secrets.report_snapshot_catalog') IS NOT NULL THEN
  EXECUTE 'SET LOCAL ROLE board_report_snapshot_owner';
  PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
  IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN
   RAISE EXCEPTION 'Report capture is not ready before restrictions';
  END IF;
  EXECUTE 'RESET ROLE';
 END IF;
END $restriction_preflight$;

CREATE SCHEMA restrictions;
REVOKE ALL ON SCHEMA restrictions FROM PUBLIC;
GRANT USAGE ON SCHEMA restrictions,content,post_secrets,staff_identity TO board_restriction_owner;
GRANT SELECT(slug,staff_only),UPDATE(slug),REFERENCES(slug) ON content.boards TO board_restriction_owner;
CREATE POLICY restriction_board_read ON content.boards FOR SELECT TO board_restriction_owner USING(true);
CREATE POLICY restriction_board_lock ON content.boards FOR UPDATE TO board_restriction_owner USING(true) WITH CHECK(true);
GRANT SELECT(id,board,thread_id,deleted),UPDATE(id) ON content.posts TO board_restriction_owner;
GRANT SELECT(id,board,deleted,archived_at),UPDATE(id) ON content.threads TO board_restriction_owner;
GRANT SELECT(post_id,board,actor_hash,automatic_identity) ON post_secrets.posting_history TO board_restriction_owner;
GRANT SELECT(token_hash,expires_at,verified_level) ON post_secrets.anonymous_sessions TO board_restriction_owner;
GRANT SELECT(id,role,revoked_at,allow_boards,deny_boards),UPDATE(id) ON staff_identity.accounts TO board_restriction_owner;
GRANT SELECT(token_hash,csrf_hash,account_id,credential_id,authenticated_at,expires_at,last_activity_at),UPDATE(token_hash)
 ON staff_identity.sessions TO board_restriction_owner;
GRANT SELECT(id,account_id),UPDATE(id) ON staff_identity.credentials TO board_restriction_owner;
GRANT EXECUTE ON FUNCTION staff_identity.has_board_access(bigint,text) TO board_restriction_owner;
SET ROLE board_posting_cooldown_owner;
GRANT EXECUTE ON FUNCTION content.lock_posting_actor(bytea,boolean) TO board_restriction_owner;
RESET ROLE;
SET ROLE board_anonymous_owner;
GRANT EXECUTE ON FUNCTION post_secrets.lookup_automatic_identity(bytea,bigint) TO board_restriction_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA restrictions,content TO board_restriction_owner;
SET ROLE board_restriction_owner;

CREATE TABLE restrictions.capacity (
 singleton boolean PRIMARY KEY CHECK(singleton),
 byte_limit bigint NOT NULL DEFAULT 67108864 CHECK(byte_limit BETWEEN 1 AND 1073741824),
 used_bytes bigint NOT NULL DEFAULT 0 CHECK(used_bytes BETWEEN 0 AND byte_limit),
 row_limit integer NOT NULL DEFAULT 100000 CHECK(row_limit BETWEEN 1 AND 1000000)
);
INSERT INTO restrictions.capacity(singleton) VALUES(true);
CREATE TABLE restrictions.bans (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK(id>0),
 board text NOT NULL REFERENCES content.boards(slug),
 post_id bigint NOT NULL CHECK(post_id>0),
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 automatic_identity uuid,
 global boolean NOT NULL,
 warning boolean NOT NULL,
 starts_at timestamptz NOT NULL CHECK(isfinite(starts_at)),
 ends_at timestamptz CHECK(ends_at IS NULL OR isfinite(ends_at)),
 public_reason text NOT NULL CHECK(octet_length(public_reason) BETWEEN 1 AND 16384),
 private_reason text NOT NULL CHECK(octet_length(private_reason)<=32768),
 account_id bigint NOT NULL CHECK(account_id>0),
 active boolean NOT NULL DEFAULT true,
 retired_at timestamptz,
 retired_by bigint,
 retirement text CHECK(retirement IN('unban','expiration','warning-viewed')),
 charged_bytes bigint NOT NULL CHECK(charged_bytes>=512),
 CHECK((warning AND ends_at IS NOT NULL AND ends_at=starts_at) OR (NOT warning AND (ends_at IS NULL OR ends_at>starts_at))),
 CHECK((active AND retired_at IS NULL AND retired_by IS NULL AND retirement IS NULL)
    OR (NOT active AND retired_at IS NOT NULL AND retirement IS NOT NULL))
);
CREATE INDEX restriction_address_active ON restrictions.bans(actor_hash,id) WHERE active;
CREATE INDEX restriction_automatic_active ON restrictions.bans(automatic_identity,id) WHERE active AND automatic_identity IS NOT NULL;
CREATE INDEX restriction_board_history ON restrictions.bans(board,id DESC);
CREATE TABLE restrictions.audit (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK(id>0),
 ban_id bigint NOT NULL REFERENCES restrictions.bans(id),
 action text NOT NULL CHECK(action IN('ban','warning','unban','expiration','warning-viewed')),
 account_id bigint CHECK(account_id>0),
 happened_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE restrictions.catalog (
 singleton boolean PRIMARY KEY CHECK(singleton), definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
REVOKE ALL ON restrictions.capacity,restrictions.bans,restrictions.audit,restrictions.catalog FROM PUBLIC;

CREATE FUNCTION restrictions.invoker() RETURNS text LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END
$$;
CREATE FUNCTION restrictions.staff_authority(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_recent boolean)
RETURNS bigint LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_account bigint; v_credential bytea; v_role text; v_recent boolean;
BEGIN
 IF p_session IS NULL OR octet_length(p_session)<>32 OR p_csrf IS NULL OR octet_length(p_csrf)<>32
    OR p_idle IS NULL OR p_idle NOT BETWEEN 60 AND 3600 OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
    OR p_recent IS NULL THEN
  RAISE EXCEPTION 'Restriction authority is unavailable' USING ERRCODE='P134A';
 END IF;
 SELECT s.account_id,s.credential_id INTO v_account,v_credential FROM staff_identity.sessions s WHERE s.token_hash=p_session;
 IF NOT FOUND THEN RAISE EXCEPTION 'Restriction authority is unavailable' USING ERRCODE='P134A'; END IF;
 PERFORM a.id FROM staff_identity.accounts a WHERE a.id=v_account FOR SHARE NOWAIT;
 PERFORM s.token_hash FROM staff_identity.sessions s WHERE s.token_hash=p_session FOR SHARE NOWAIT;
 PERFORM c.id FROM staff_identity.credentials c WHERE c.id=v_credential FOR KEY SHARE NOWAIT;
 SELECT a.role,s.authenticated_at>clock_timestamp()-interval '10 minutes' INTO v_role,v_recent
 FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id
 JOIN staff_identity.credentials c ON c.id=s.credential_id AND c.account_id=a.id
 WHERE s.token_hash=p_session AND s.account_id=v_account AND s.credential_id=v_credential
   AND s.csrf_hash=p_csrf AND s.expires_at>clock_timestamp()
   AND s.last_activity_at>clock_timestamp()-make_interval(secs=>p_idle)
   AND a.revoked_at IS NULL AND a.role IN('moderator','manager','admin')
   AND staff_identity.has_board_access(a.id,p_board);
 IF NOT FOUND OR (p_board='j' AND v_role='moderator') THEN
  RAISE EXCEPTION 'Restriction authority is unavailable' USING ERRCODE='P134A';
 END IF;
 IF p_recent AND NOT v_recent THEN RAISE EXCEPTION 'Recent authentication is required' USING ERRCODE='P134Q'; END IF;
 RETURN v_account;
END $$;

CREATE FUNCTION restrictions.acl(p_acl aclitem[]) RETURNS jsonb LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce(jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
    CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
    ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,
    a.privilege_type,a.is_grantable),'[]'::jsonb) FROM aclexplode(p_acl) a
$$;
CREATE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions'
 ), triggers AS (
  SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'post_secrets.posting_history'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='post_secrets.posting_history'::regclass AND t.tgname='source_restriction_guard' AND NOT t.tgisinternal
 ), funcs AS (
  SELECT p.* FROM pg_proc p WHERE p.proowner='board_restriction_owner'::regrole
    OR p.oid IN(SELECT t.tgfoid FROM triggers t)
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
      'post_secrets.lookup_automatic_identity(bytea,bigint)'::regprocedure)),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),restrictions.acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
   FROM pg_namespace n WHERE n.nspname='restrictions'))::text,'UTF8'))
$$;
CREATE FUNCTION content.restrictions_ready() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname='board_restriction_owner'
    AND NOT(r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.roleid='board_restriction_owner'::regrole
    AND (m.member<>'board_migrator'::regrole OR m.inherit_option OR m.admin_option OR NOT m.set_option))
 AND EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.roleid='board_restriction_owner'::regrole
    AND m.member='board_migrator'::regrole AND NOT m.inherit_option AND NOT m.admin_option AND m.set_option)
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.member='board_restriction_owner'::regrole)
 AND NOT EXISTS(SELECT 1 FROM pg_roles r
    WHERE r.rolname IN('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor')
      AND (pg_has_role(r.oid,'board_posting_cooldown_owner','MEMBER')
        OR pg_has_role(r.oid,'board_thread_transfer_owner','MEMBER')
        OR has_any_column_privilege(r.oid,'post_secrets.posting_history','INSERT,UPDATE')))
 AND NOT has_schema_privilege('board_restriction_owner','restrictions','CREATE')
 AND NOT has_schema_privilege('board_restriction_owner','content','CREATE')
 AND (SELECT definition_hash=restrictions.catalog_hash() FROM restrictions.catalog WHERE singleton)
$$;
CREATE FUNCTION restrictions.gate(p_write boolean DEFAULT true) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF p_write IS NULL THEN RAISE EXCEPTION 'Invalid restriction lock' USING ERRCODE='23514'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Restrictions require Read Committed' USING ERRCODE='P134I';
 END IF;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restrictions are unavailable' USING ERRCODE='P134R';
 END IF;
 IF p_write THEN
  PERFORM singleton FROM restrictions.capacity WHERE singleton FOR UPDATE NOWAIT;
 ELSE
  PERFORM singleton FROM restrictions.capacity WHERE singleton FOR SHARE NOWAIT;
 END IF;
 IF NOT FOUND THEN RAISE EXCEPTION 'Restrictions are unavailable' USING ERRCODE='P134R'; END IF;
END $$;

-- Independently tested against the executed source. This helper has no grant
-- to application roles and cannot confer a Pass or verified-user capability.
CREATE FUNCTION restrictions.source_decision(p_rows jsonb,p_board text,p_request bigint,
 p_pass_present boolean,p_bypass boolean,p_verified boolean)
RETURNS TABLE(disposition text,expired bigint[]) LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE r record;
BEGIN
 IF p_rows IS NULL OR jsonb_typeof(p_rows)<>'array' OR jsonb_array_length(p_rows)>1000
    OR p_board IS NULL OR p_request IS NULL OR p_pass_present IS NULL OR p_bypass IS NULL OR p_verified IS NULL THEN
  RAISE EXCEPTION 'Invalid restriction decision' USING ERRCODE='23514';
 END IF;
 disposition:='allowed'; expired:=ARRAY[]::bigint[];
 FOR r IN SELECT * FROM jsonb_to_recordset(p_rows) AS x(
    no bigint,key text,board text,global boolean,starts_on bigint,ends_on bigint,
    template_id bigint,admin text,reason text) LOOP
  IF (r.key='host' AND p_pass_present AND p_bypass) OR (r.key='4pass_id' AND NOT p_pass_present) THEN CONTINUE; END IF;
  IF r.ends_on IS NOT NULL AND r.ends_on<>0 THEN
   IF r.ends_on<=r.starts_on THEN disposition:='warned'; EXIT; END IF;
   IF r.ends_on<=p_request THEN expired:=array_append(expired,r.no); CONTINUE; END IF;
  END IF;
  IF r.key='4pass_id' AND r.template_id=124 THEN CONTINUE; END IF;
  IF p_verified AND r.key='host' AND r.admin='Auto-ban' AND strpos(r.reason,'Proxy')>0 THEN CONTINUE; END IF;
  IF r.global OR r.board=p_board THEN disposition:='banned'; EXIT; END IF;
 END LOOP;
 RETURN NEXT;
END $$;

CREATE FUNCTION restrictions.posting_decision(p_actor bytea,p_token bytea,p_board text,p_request bigint)
RETURNS TABLE(disposition text,expired bigint[]) LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_automatic uuid; v_new boolean; v_verified boolean:=false; v_rows jsonb; v_count integer;
BEGIN
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR (p_token IS NOT NULL AND octet_length(p_token)<>32)
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_request IS NULL OR p_request<=0 THEN
  RAISE EXCEPTION 'Invalid restriction context' USING ERRCODE='23514';
 END IF;
 -- Empty state has no session/time decision. Historical controlled fixtures
 -- and operator imports do not manufacture a current anonymous capability.
 IF NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE active) THEN
  RETURN QUERY SELECT 'allowed'::text,ARRAY[]::bigint[]; RETURN;
 END IF;
 SELECT a.automatic_identity,a.source_new INTO v_automatic,v_new
    FROM post_secrets.lookup_automatic_identity(p_token,p_request) a;
 IF v_new THEN v_automatic:=NULL;
 ELSE
  SELECT coalesce(s.verified_level>0,false) INTO v_verified FROM post_secrets.anonymous_sessions s
    WHERE s.token_hash=p_token AND s.expires_at>extract(epoch FROM clock_timestamp())::bigint;
  v_verified:=coalesce(v_verified,false);
 END IF;
 -- Source queries password first, then address. A row matching both fields
 -- is visited twice, including duplicate expired IDs, just like the source.
 WITH matches AS (
  SELECT b.*,0 AS position,'password'::text AS key FROM restrictions.bans b
    WHERE b.active AND v_automatic IS NOT NULL AND b.automatic_identity=v_automatic
  UNION ALL
  SELECT b.*,2 AS position,'host'::text AS key FROM restrictions.bans b WHERE b.active AND b.actor_hash=p_actor
 ), bounded AS (SELECT * FROM matches ORDER BY position,id LIMIT 1001)
 SELECT count(*),coalesce(jsonb_agg(jsonb_build_object('no',id,'key',key,'board',board,'global',global,
    'starts_on',floor(extract(epoch FROM starts_at))::bigint,
    'ends_on',CASE WHEN ends_at IS NULL THEN 0 ELSE floor(extract(epoch FROM ends_at))::bigint END,
    'template_id',0,'admin','Staff','reason','') ORDER BY position,id),'[]'::jsonb)
 INTO v_count,v_rows FROM bounded;
 IF v_count>1000 THEN RAISE EXCEPTION 'Restriction context exceeds its bound' USING ERRCODE='P134C'; END IF;
 -- Pass and automatic-proxy issuers are not yet installed. No request flag can
 -- invent these trusted exceptions; their pure source policy is tested above.
 RETURN QUERY SELECT d.disposition,d.expired FROM restrictions.source_decision(v_rows,p_board,p_request,false,false,v_verified) d;
END $$;

CREATE FUNCTION content.evaluate_posting_restrictions(p_actor bytea,p_token bytea,p_board text,p_request bigint)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_state text; v_expired bigint[]; v_id bigint;
BEGIN
 IF restrictions.invoker() NOT IN('board_public','board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Restriction evaluation is unavailable' USING ERRCODE='42501';
 END IF;
 PERFORM restrictions.gate(false);
 SELECT d.disposition,d.expired INTO v_state,v_expired FROM restrictions.posting_decision(p_actor,p_token,p_board,p_request) d;
 IF cardinality(v_expired)>0 THEN
  PERFORM restrictions.gate(true);
  FOR v_id IN UPDATE restrictions.bans SET active=false,retired_at=clock_timestamp(),retirement='expiration'
     WHERE id=ANY(v_expired) AND active AND NOT warning AND floor(extract(epoch FROM ends_at))<=p_request
     RETURNING id LOOP
   INSERT INTO restrictions.audit(ban_id,action) VALUES(v_id,'expiration');
  END LOOP;
 END IF;
 RETURN v_state;
END $$;

-- Public status and viewing are a documented replacement contract: the
-- supplied snapshot contains the warning redirect but no /banned controller.
CREATE FUNCTION content.restriction_status(p_actor bytea,p_token bytea,p_board text,p_request bigint)
RETURNS TABLE(id bigint,board text,global boolean,warning boolean,starts_at timestamptz,
 ends_at timestamptz,public_reason text) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_automatic uuid;
BEGIN
 IF restrictions.invoker() NOT IN('board_public','board_migrator') THEN
  RAISE EXCEPTION 'Restriction status is unavailable' USING ERRCODE='42501';
 END IF;
 PERFORM content.evaluate_posting_restrictions(p_actor,p_token,p_board,p_request);
 -- Status explains report restrictions too: valid retained equality applies
 -- during creation-second/idle reset, even though posting ignores it then.
 SELECT a.automatic_identity INTO v_automatic
  FROM post_secrets.lookup_automatic_identity(p_token,p_request) a;
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans b WHERE b.active
    AND (b.actor_hash=p_actor OR (v_automatic IS NOT NULL AND b.automatic_identity=v_automatic))
    AND (b.warning OR b.global OR b.board=p_board)
    LIMIT 65) bounded)>64 THEN
  RAISE EXCEPTION 'Restriction status exceeds its bound' USING ERRCODE='P134C';
 END IF;
 RETURN QUERY SELECT b.id,b.board,b.global,b.warning,b.starts_at,b.ends_at,b.public_reason
  FROM restrictions.bans b WHERE b.active
    AND (b.actor_hash=p_actor OR (v_automatic IS NOT NULL AND b.automatic_identity=v_automatic))
    AND (b.warning OR b.global OR b.board=p_board)
  ORDER BY b.id LIMIT 64;
END $$;

CREATE FUNCTION content.acknowledge_warning(p_actor bytea,p_token bytea,p_board text,p_request bigint,p_id bigint)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_automatic uuid;
BEGIN
 IF restrictions.invoker() NOT IN('board_public','board_migrator') THEN
  RAISE EXCEPTION 'Warning viewing is unavailable' USING ERRCODE='42501';
 END IF;
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
    OR p_id IS NULL OR p_id<=0 THEN
  RAISE EXCEPTION 'Invalid warning context' USING ERRCODE='23514';
 END IF;
 PERFORM restrictions.gate();
 SELECT a.automatic_identity INTO v_automatic
  FROM post_secrets.lookup_automatic_identity(p_token,p_request) a;
 UPDATE restrictions.bans b SET active=false,retired_at=clock_timestamp(),retirement='warning-viewed'
  WHERE b.id=p_id AND b.active AND b.warning
    AND (b.actor_hash=p_actor OR (v_automatic IS NOT NULL AND b.automatic_identity=v_automatic));
 IF NOT FOUND THEN RETURN false; END IF;
 INSERT INTO restrictions.audit(ban_id,action) VALUES(p_id,'warning-viewed');
 RETURN true;
END $$;

CREATE FUNCTION content.assert_posting_restrictions(p_actor bytea,p_token bytea,p_board text,p_request bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_state text;
BEGIN
 v_state:=content.evaluate_posting_restrictions(p_actor,p_token,p_board,p_request);
 IF v_state='warned' THEN RAISE EXCEPTION 'Error: You were warned. You must first view this warning to post again.' USING ERRCODE='P134W'; END IF;
 IF v_state='banned' THEN RAISE EXCEPTION 'Error: You are banned.' USING ERRCODE='P134B'; END IF;
 IF v_state<>'allowed' OR v_state IS NULL THEN RAISE EXCEPTION 'Restriction state is unavailable' USING ERRCODE='P134R'; END IF;
END $$;

-- SECURITY INVOKER preserves the real internal caller. Runtime roles cannot
-- claim the transfer owner's identity or bypass its separately sealed handoff.
CREATE FUNCTION content.check_inserted_restriction() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_token text; v_token_hash bytea;
BEGIN
 IF current_user IN('board_migrator','board_thread_transfer_owner') THEN RETURN NEW; END IF;
 IF current_user<>'board_posting_cooldown_owner' THEN
  RAISE EXCEPTION 'Posting restriction registration is unavailable' USING ERRCODE='42501';
 END IF;
 v_token:=current_setting('board.restriction_token',true);
 IF coalesce(v_token,'')<>'' THEN
  IF octet_length(v_token)<>64 OR v_token !~ '^[0-9a-f]{64}$' THEN RAISE EXCEPTION 'Invalid restriction token' USING ERRCODE='23514'; END IF;
  v_token_hash:=decode(v_token,'hex');
 END IF;
 PERFORM content.assert_posting_restrictions(NEW.actor_hash,v_token_hash,NEW.board,NEW.request_at);
 RETURN NEW;
END $$;

CREATE FUNCTION content.issue_post_restriction(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_post bigint,
 p_global boolean,p_warning boolean,p_seconds bigint,p_public text,p_private text)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_actor bytea; v_automatic uuid; v_account bigint; v_id bigint; v_now timestamptz; v_charge bigint; v_thread bigint;
BEGIN
 IF restrictions.invoker() NOT IN('board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Restriction issuance is unavailable' USING ERRCODE='42501';
 END IF;
 IF p_post IS NULL OR p_post<=0 OR p_global IS NULL OR p_warning IS NULL
    OR p_public IS NULL OR octet_length(p_public) NOT BETWEEN 1 AND 16384 OR p_private IS NULL
    OR octet_length(p_private)>32768 OR (p_warning AND p_seconds IS DISTINCT FROM 0)
    OR (NOT p_warning AND p_seconds IS NOT NULL AND p_seconds NOT BETWEEN 1 AND 863913600) THEN
  RAISE EXCEPTION 'Invalid restriction' USING ERRCODE='23514';
 END IF;
 v_account:=restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,true);
 SELECT h.actor_hash,h.automatic_identity INTO v_actor,v_automatic FROM post_secrets.posting_history h
    WHERE h.board=p_board AND h.post_id=p_post;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post identity is unavailable' USING ERRCODE='P134N'; END IF;
 -- Resolve the private actor before its gate; revalidate after parent locks.
 PERFORM content.lock_posting_actor(v_actor,false);
 PERFORM restrictions.gate();
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR SHARE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 SELECT p.thread_id INTO v_thread FROM content.posts p WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 PERFORM t.id FROM content.threads t WHERE t.board=p_board AND t.id=v_thread AND NOT t.deleted
    AND t.archived_at IS NULL FOR SHARE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 PERFORM p.id FROM content.posts p JOIN post_secrets.posting_history h ON h.post_id=p.id AND h.board=p.board
    WHERE p.id=p_post AND p.board=p_board AND NOT p.deleted AND h.actor_hash=v_actor
       AND h.automatic_identity IS NOT DISTINCT FROM v_automatic FOR SHARE OF p NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post identity changed' USING ERRCODE='P134N'; END IF;
 v_charge:=512+octet_length(p_public)::bigint+octet_length(p_private)::bigint;
 IF EXISTS(SELECT 1 FROM restrictions.capacity c WHERE c.singleton AND (c.used_bytes>c.byte_limit-v_charge
    OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans LIMIT 1000001) bounded)>=c.row_limit)) THEN
  RAISE EXCEPTION 'Restriction capacity is exhausted' USING ERRCODE='P134C';
 END IF;
 v_now:=date_trunc('second',clock_timestamp());
 INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,starts_at,ends_at,
    public_reason,private_reason,account_id,charged_bytes)
 VALUES(p_board,p_post,v_actor,v_automatic,p_global,p_warning,v_now,
    CASE WHEN p_warning THEN v_now WHEN p_seconds IS NULL THEN NULL ELSE v_now+make_interval(secs=>p_seconds::double precision) END,
    p_public,p_private,v_account,v_charge) RETURNING id INTO v_id;
 UPDATE restrictions.capacity SET used_bytes=used_bytes+v_charge WHERE singleton;
 INSERT INTO restrictions.audit(ban_id,action,account_id) VALUES(v_id,CASE WHEN p_warning THEN 'warning' ELSE 'ban' END,v_account);
 -- Time-based validity is checked again after all contended content locks.
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,true);
 RETURN v_id;
END $$;

CREATE FUNCTION content.unban_restriction(p_session bytea,p_csrf bytea,p_idle integer,p_id bigint)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_board text; v_account bigint;
BEGIN
 IF restrictions.invoker() NOT IN('board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Unban is unavailable' USING ERRCODE='42501';
 END IF;
 IF p_id IS NULL OR p_id<=0 THEN RAISE EXCEPTION 'Invalid restriction' USING ERRCODE='23514'; END IF;
 PERFORM restrictions.gate();
 SELECT b.board INTO v_board FROM restrictions.bans b WHERE b.id=p_id;
 IF NOT FOUND THEN RAISE EXCEPTION 'Restriction is unavailable' USING ERRCODE='P134N'; END IF;
 v_account:=restrictions.staff_authority(p_session,p_csrf,p_idle,v_board,true);
 UPDATE restrictions.bans SET active=false,retired_at=clock_timestamp(),retired_by=v_account,retirement='unban'
    WHERE id=p_id AND active;
 IF NOT FOUND THEN RETURN false; END IF;
 INSERT INTO restrictions.audit(ban_id,action,account_id) VALUES(p_id,'unban',v_account);
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,v_board,true);
 RETURN true;
END $$;

REVOKE ALL ON FUNCTION restrictions.invoker(),restrictions.staff_authority(bytea,bytea,integer,text,boolean),
 restrictions.acl(aclitem[]),restrictions.catalog_hash(),restrictions.gate(boolean),content.restrictions_ready(),
 restrictions.source_decision(jsonb,text,bigint,boolean,boolean,boolean),restrictions.posting_decision(bytea,bytea,text,bigint),
 content.evaluate_posting_restrictions(bytea,bytea,text,bigint),content.assert_posting_restrictions(bytea,bytea,text,bigint),
 content.restriction_status(bytea,bytea,text,bigint),content.acknowledge_warning(bytea,bytea,text,bigint,bigint),
 content.check_inserted_restriction(),
 content.issue_post_restriction(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text),
 content.unban_restriction(bytea,bytea,integer,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.restrictions_ready() TO board_public,board_staff,board_auth;
GRANT EXECUTE ON FUNCTION content.evaluate_posting_restrictions(bytea,bytea,text,bigint)
 TO board_public,board_staff;
GRANT EXECUTE ON FUNCTION content.restriction_status(bytea,bytea,text,bigint),
 content.acknowledge_warning(bytea,bytea,text,bigint,bigint) TO board_public;
GRANT EXECUTE ON FUNCTION content.assert_posting_restrictions(bytea,bytea,text,bigint)
 TO board_posting_cooldown_owner;
GRANT EXECUTE ON FUNCTION content.check_inserted_restriction() TO board_migrator;
GRANT EXECUTE ON FUNCTION content.issue_post_restriction(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text),
 content.unban_restriction(bytea,bytea,integer,bigint) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,content FROM board_restriction_owner;
CREATE TRIGGER source_restriction_guard AFTER INSERT ON post_secrets.posting_history
 FOR EACH ROW EXECUTE FUNCTION content.check_inserted_restriction();
DO $restriction_reseal$
BEGIN
 IF to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
  EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
  UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
  IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
   RAISE EXCEPTION 'Transfer catalog did not qualify after restrictions';
  END IF;
  EXECUTE 'RESET ROLE';
 END IF;
END $restriction_reseal$;
SET ROLE board_restriction_owner;
INSERT INTO restrictions.catalog VALUES(true,restrictions.catalog_hash());
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction catalog did not qualify'; END IF; END $$;
RESET ROLE;
