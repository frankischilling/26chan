-- Bounded native storage for the source's controller-less submission templates.
-- No runtime has table access. Workflow APIs and proof consumption are separate.
-- Bootstrap board_submission_owner before applying this migration.
DO $$
DECLARE r record;
BEGIN
 SELECT * INTO r FROM pg_roles WHERE rolname='board_submission_owner';
 IF NOT FOUND OR r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole
  OR r.rolreplication OR r.rolbypassrls OR r.rolconfig IS NOT NULL THEN
  RAISE EXCEPTION 'Submission function owner is missing or unsafe';
 END IF;
 IF (SELECT count(*) FROM pg_auth_members WHERE roleid=r.oid)<>1 OR NOT EXISTS(
  SELECT 1 FROM pg_auth_members m JOIN pg_roles member ON member.oid=m.member
  WHERE m.roleid=r.oid AND member.rolname='board_migrator' AND m.set_option
   AND NOT m.inherit_option AND NOT m.admin_option)
  OR EXISTS(SELECT 1 FROM pg_auth_members WHERE member=r.oid)
  OR EXISTS(SELECT 1 FROM pg_class WHERE relowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_proc WHERE proowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_namespace WHERE nspowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_class c,LATERAL aclexplode(c.relacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_attribute c,LATERAL aclexplode(c.attacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_proc c,LATERAL aclexplode(c.proacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_namespace c,LATERAL aclexplode(c.nspacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_database c,LATERAL aclexplode(c.datacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_default_acl c WHERE c.defaclrole=r.oid)
  OR EXISTS(SELECT 1 FROM pg_default_acl c,LATERAL aclexplode(c.defaclacl) a WHERE a.grantee=r.oid)
 THEN RAISE EXCEPTION 'Submission owner has unexpected existing authority'; END IF;
 IF to_regnamespace('submission_private') IS NOT NULL OR to_regnamespace('submission_api') IS NOT NULL THEN
  RAISE EXCEPTION 'Submission schema already exists';
 END IF;
END $$;
CREATE SCHEMA submission_private AUTHORIZATION board_migrator;
CREATE SCHEMA submission_api AUTHORIZATION board_migrator;
REVOKE ALL ON SCHEMA submission_private,submission_api FROM PUBLIC;

-- These helpers are pure constraint predicates, not runtime APIs.
CREATE FUNCTION submission_private.valid_text(p_value text,p_max integer,p_required boolean,p_multiline boolean)
RETURNS boolean LANGUAGE sql IMMUTABLE STRICT SET search_path=pg_catalog,pg_temp AS $$
 SELECT p_max BETWEEN 1 AND 16000 AND octet_length(p_value)<=p_max*4
 AND (SELECT coalesce(sum(CASE WHEN ascii(c)>65535 THEN 2 ELSE 1 END),0)
      FROM regexp_split_to_table(p_value,'') c WHERE c<>'')<=p_max
 AND (NOT p_required OR length(btrim(p_value,U&' \0009\000A\000B\000C\000D\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000'))>0)
 AND NOT EXISTS(SELECT 1 FROM regexp_split_to_table(p_value,'') c
   WHERE c<>'' AND (ascii(c)<32 OR ascii(c) BETWEEN 127 AND 159)
   AND NOT(p_multiline AND ascii(c) IN(9,10,13)))
$$;
CREATE FUNCTION submission_private.valid_text_array(p_values text[],p_count integer,p_max integer,p_multiline boolean)
RETURNS boolean LANGUAGE sql IMMUTABLE STRICT SET search_path=pg_catalog,pg_temp AS $$
 SELECT cardinality(p_values)=p_count AND array_ndims(p_values)=1 AND array_lower(p_values,1)=1
 AND array_position(p_values,NULL) IS NULL
 AND NOT EXISTS(SELECT 1 FROM unnest(p_values) value WHERE NOT submission_private.valid_text(value,p_max,true,p_multiline))
$$;
CREATE FUNCTION submission_private.valid_boards(p_values text[])
RETURNS boolean LANGUAGE sql IMMUTABLE STRICT SET search_path=pg_catalog,pg_temp AS $$
 SELECT cardinality(p_values) BETWEEN 1 AND 128 AND array_ndims(p_values)=1 AND array_lower(p_values,1)=1
 AND array_position(p_values,NULL) IS NULL
 AND NOT EXISTS(SELECT 1 FROM unnest(p_values) value WHERE value='polls' OR value !~ '^[a-z0-9]{1,10}$')
 AND (SELECT count(DISTINCT value) FROM unnest(p_values) value)=cardinality(p_values)
$$;
REVOKE ALL ON FUNCTION submission_private.valid_text(text,integer,boolean,boolean),
 submission_private.valid_text_array(text[],integer,integer,boolean),submission_private.valid_boards(text[]) FROM PUBLIC;

CREATE FUNCTION submission_private.valid_email(p_value text,p_max integer)
RETURNS boolean LANGUAGE sql IMMUTABLE STRICT SET search_path=pg_catalog,pg_temp AS $$
 SELECT p_max BETWEEN 3 AND 254 AND octet_length(p_value) BETWEEN 3 AND p_max
 AND p_value !~ '[^!-~]' AND length(p_value)-length(replace(p_value,'@',''))=1
 AND length(split_part(p_value,'@',1)) BETWEEN 1 AND 64
 AND split_part(p_value,'@',1) ~ '^[A-Za-z0-9.!#$%&''*+/=?^_`{|}~-]+$'
 AND left(p_value,1)<>'.' AND right(split_part(p_value,'@',1),1)<>'.'
 AND strpos(split_part(p_value,'@',1),'..')=0
 AND strpos(split_part(p_value,'@',2),'.')>0
 AND NOT EXISTS(SELECT 1 FROM regexp_split_to_table(split_part(p_value,'@',2),'\.') label
  WHERE length(label) NOT BETWEEN 1 AND 63 OR label !~ '^[A-Za-z0-9-]+$'
   OR left(label,1)='-' OR right(label,1)='-')
$$;
REVOKE ALL ON FUNCTION submission_private.valid_email(text,integer) FROM PUBLIC;

CREATE TABLE submission_private.feedback_profiles (
 id bigint PRIMARY KEY CHECK(id>0), enabled boolean NOT NULL DEFAULT false,
 subject_limit integer NOT NULL CHECK(subject_limit BETWEEN 1 AND 512),
 message_limit integer NOT NULL CHECK(message_limit BETWEEN 1 AND 16000),
 capacity integer NOT NULL CHECK(capacity BETWEEN 1 AND 100000),
 received integer NOT NULL DEFAULT 0 CHECK(received BETWEEN 0 AND capacity)
);
CREATE UNIQUE INDEX one_enabled_feedback_profile ON submission_private.feedback_profiles(enabled) WHERE enabled;
CREATE TABLE submission_private.feedback_categories (
 profile_id bigint NOT NULL REFERENCES submission_private.feedback_profiles(id),
 id integer NOT NULL CHECK(id BETWEEN 0 AND 65535),
 ordinal smallint NOT NULL CHECK(ordinal BETWEEN 0 AND 31),
 label text NOT NULL CHECK(submission_private.valid_text(label,100,true,false)),
 short_label text NOT NULL CHECK(submission_private.valid_text(short_label,32,true,false)),
 PRIMARY KEY(profile_id,id), UNIQUE(profile_id,ordinal)
);
CREATE TABLE submission_private.feedback (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 profile_id bigint NOT NULL, category integer NOT NULL,
 subject text NOT NULL CHECK(submission_private.valid_text(subject,512,true,false)),
 message text NOT NULL CHECK(submission_private.valid_text(message,16000,true,true)),
 state smallint NOT NULL DEFAULT 0 CHECK(state BETWEEN 0 AND 3),
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 peer inet NOT NULL,
 user_agent text NOT NULL CHECK(submission_private.valid_text(user_agent,512,false,false)),
 language text NOT NULL CHECK(submission_private.valid_text(language,128,false,false)),
 membership_hash bytea CHECK(membership_hash IS NULL OR octet_length(membership_hash)=32),
 created_at bigint NOT NULL CHECK(created_at BETWEEN 0 AND 253402300799),
 updated_at bigint NOT NULL CHECK(updated_at BETWEEN created_at AND 253402300799),
 reply text CHECK(reply IS NULL OR submission_private.valid_text(reply,16000,true,true)),
 reply_capcode text CHECK(reply_capcode IS NULL OR reply_capcode IN('admin','mod')),
 FOREIGN KEY(profile_id,category) REFERENCES submission_private.feedback_categories(profile_id,id)
);
CREATE INDEX feedback_review ON submission_private.feedback(profile_id,state,id DESC);
CREATE INDEX feedback_actor ON submission_private.feedback(profile_id,actor_hash,created_at DESC);

CREATE TABLE submission_private.application_campaigns (
 id bigint PRIMARY KEY CHECK(id>0), enabled boolean NOT NULL DEFAULT false,
 starts_at bigint NOT NULL CHECK(starts_at BETWEEN 0 AND 253402300798),
 ends_at bigint NOT NULL CHECK(ends_at>starts_at AND ends_at<=253402300799),
 boards text[] NOT NULL CHECK(submission_private.valid_boards(boards)),
 questions text[] NOT NULL CHECK(submission_private.valid_text_array(questions,4,1000,false)),
 field_limit integer NOT NULL CHECK(field_limit BETWEEN 1 AND 512),
 answer_limit integer NOT NULL CHECK(answer_limit BETWEEN 1 AND 16000),
 capacity integer NOT NULL CHECK(capacity BETWEEN 1 AND 100000),
 received integer NOT NULL DEFAULT 0 CHECK(received BETWEEN 0 AND capacity)
);
CREATE UNIQUE INDEX one_enabled_application_campaign ON submission_private.application_campaigns(enabled) WHERE enabled;
CREATE TABLE submission_private.applications (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 campaign_id bigint NOT NULL REFERENCES submission_private.application_campaigns(id),
 state smallint NOT NULL DEFAULT 0 CHECK(state BETWEEN 0 AND 4),
 age smallint NOT NULL CHECK(age BETWEEN 1 AND 150),
 timezone smallint NOT NULL CHECK(timezone BETWEEN -12 AND 13),
 hours smallint NOT NULL CHECK(hours BETWEEN 1 AND 24),
 times text NOT NULL CHECK(submission_private.valid_text(times,512,true,false)),
 board1 text NOT NULL CHECK(board1 ~ '^[a-z0-9]{1,10}$'),
 board2 text CHECK(board2 IS NULL OR board2 ~ '^[a-z0-9]{1,10}$'),
 answers text[] NOT NULL CHECK(submission_private.valid_text_array(answers,4,16000,true)),
 revision integer NOT NULL DEFAULT 1 CHECK(revision BETWEEN 1 AND 1000000),
 created_at bigint NOT NULL CHECK(created_at BETWEEN 0 AND 253402300799),
 updated_at bigint NOT NULL CHECK(updated_at BETWEEN created_at AND 253402300799),
 UNIQUE(id,campaign_id)
);
CREATE INDEX application_review ON submission_private.applications(campaign_id,state,id DESC);
-- Identity is physically separate from the blind-review relation.
CREATE TABLE submission_private.application_identity (
 application_id bigint PRIMARY KEY,
 campaign_id bigint NOT NULL,
 ownership_hash bytea NOT NULL UNIQUE CHECK(octet_length(ownership_hash)=32),
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 firstname text NOT NULL CHECK(submission_private.valid_text(firstname,512,true,false)),
 handle text NOT NULL CHECK(submission_private.valid_text(handle,512,true,false)),
 email text NOT NULL CHECK(submission_private.valid_email(email,254)),
 email_identity text NOT NULL CHECK(email_identity=lower(email) AND octet_length(email_identity) BETWEEN 3 AND 254),
 FOREIGN KEY(application_id,campaign_id) REFERENCES submission_private.applications(id,campaign_id) ON DELETE CASCADE,
 UNIQUE(campaign_id,email_identity)
);

CREATE TABLE submission_private.contest_campaigns (
 id bigint PRIMARY KEY CHECK(id>0), kind smallint NOT NULL CHECK(kind IN(1,2)),
 enabled boolean NOT NULL DEFAULT false,
 boards text[] NOT NULL CHECK(submission_private.valid_boards(boards)),
 submission_starts bigint NOT NULL CHECK(submission_starts BETWEEN 0 AND 253402300798),
 submission_ends bigint NOT NULL CHECK(submission_ends>submission_starts),
 voting_starts bigint NOT NULL CHECK(voting_starts>=submission_ends),
 voting_ends bigint NOT NULL CHECK(voting_ends>voting_starts AND voting_ends<=253402300799),
 capacity integer NOT NULL CHECK(capacity BETWEEN 1 AND 100000),
 received integer NOT NULL DEFAULT 0 CHECK(received BETWEEN 0 AND capacity),
 vote_capacity integer NOT NULL CHECK(vote_capacity BETWEEN 1 AND 1000000),
 votes integer NOT NULL DEFAULT 0 CHECK(votes BETWEEN 0 AND vote_capacity),
 CHECK((kind=2 AND boards=ARRAY['all']::text[]) OR (kind=1 AND NOT('all'=ANY(boards))))
);
CREATE UNIQUE INDEX one_enabled_contest_per_kind ON submission_private.contest_campaigns(kind) WHERE enabled;
CREATE TABLE submission_private.contest_entries (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 campaign_id bigint NOT NULL REFERENCES submission_private.contest_campaigns(id),
 board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$'),
 ownership_hash bytea NOT NULL UNIQUE CHECK(octet_length(ownership_hash)=32),
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 author text NOT NULL CHECK(submission_private.valid_text(author,150,false,false)),
 email text CHECK(email IS NULL OR submission_private.valid_email(email,150)),
 state smallint NOT NULL DEFAULT 0 CHECK(state BETWEEN 0 AND 4),
 media_id text NOT NULL CHECK(media_id ~ '^[a-f0-9]{32}$'),
 score integer NOT NULL DEFAULT 0 CHECK(score BETWEEN 0 AND 1000000),
 created_at bigint NOT NULL CHECK(created_at BETWEEN 0 AND 253402300799),
 updated_at bigint NOT NULL CHECK(updated_at BETWEEN created_at AND 253402300799),
 UNIQUE(id,campaign_id,board)
);
CREATE INDEX contest_listing ON submission_private.contest_entries(campaign_id,board,state,id DESC);
CREATE TABLE submission_private.contest_votes (
 campaign_id bigint NOT NULL REFERENCES submission_private.contest_campaigns(id),
 board text NOT NULL,
 voter_hash bytea NOT NULL CHECK(octet_length(voter_hash)=32),
 entry_id bigint NOT NULL,
 created_at bigint NOT NULL CHECK(created_at BETWEEN 0 AND 253402300799),
 PRIMARY KEY(campaign_id,board,voter_hash),
 FOREIGN KEY(entry_id,campaign_id,board) REFERENCES submission_private.contest_entries(id,campaign_id,board)
);
REVOKE ALL ON ALL TABLES IN SCHEMA submission_private FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA submission_private FROM PUBLIC;
-- Leave both schemas completely closed to runtime roles until the narrow APIs
-- are implemented and qualified. Merely migrating cannot open submissions.

-- Only bounded, non-personal configuration is exposed by these read APIs.
CREATE TABLE submission_private.catalog (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
INSERT INTO submission_private.catalog VALUES(true,decode(repeat('00',32),'hex'));
GRANT USAGE ON SCHEMA submission_private,submission_api TO board_submission_owner;
GRANT SELECT ON submission_private.catalog,submission_private.feedback_profiles,
 submission_private.feedback_categories,submission_private.application_campaigns,
 submission_private.contest_campaigns TO board_submission_owner;
GRANT CREATE ON SCHEMA submission_private,submission_api TO board_submission_owner;
SET LOCAL ROLE board_submission_owner;
CREATE FUNCTION submission_private.catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT sha256(convert_to(jsonb_build_object(
 'role',(SELECT jsonb_build_array(rolname,rolcanlogin,rolsuper,rolcreatedb,rolcreaterole,
   rolreplication,rolbypassrls,rolinherit,rolconfig) FROM pg_roles WHERE rolname='board_submission_owner'),
 'membership',(SELECT coalesce(jsonb_agg(jsonb_build_array(r.rolname,u.rolname,m.admin_option,m.inherit_option,m.set_option)
   ORDER BY r.rolname,u.rolname),'[]'::jsonb) FROM pg_auth_members m
   JOIN pg_roles r ON r.oid=m.roleid JOIN pg_roles u ON u.oid=m.member
   WHERE r.rolname='board_submission_owner' OR u.rolname='board_submission_owner'),
 'owner_grants',(SELECT jsonb_agg(value ORDER BY value::text) FROM (
   SELECT jsonb_build_array('relation',n.nspname,c.relname,a.privilege_type,a.is_grantable) AS value
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(c.relacl) a
    WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')
   UNION ALL SELECT jsonb_build_array('column',n.nspname,c.relname,col.attname,a.privilege_type,a.is_grantable)
    FROM pg_attribute col JOIN pg_class c ON c.oid=col.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(col.attacl) a
    WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')
   UNION ALL SELECT jsonb_build_array('function',n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),a.privilege_type,a.is_grantable)
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace,LATERAL aclexplode(p.proacl) a
    WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')
   UNION ALL SELECT jsonb_build_array('schema',n.nspname,a.privilege_type,a.is_grantable)
    FROM pg_namespace n,LATERAL aclexplode(n.nspacl) a WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')
   UNION ALL SELECT jsonb_build_array('database',a.privilege_type,a.is_grantable)
    FROM pg_database d,LATERAL aclexplode(d.datacl) a WHERE d.datname=current_database()
     AND a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')
  ) grants),
 'default_acl',(SELECT coalesce(jsonb_agg(jsonb_build_array(pg_get_userbyid(d.defaclrole),n.nspname,d.defaclobjtype,d.defaclacl::text) ORDER BY pg_get_userbyid(d.defaclrole),n.nspname,d.defaclobjtype),'[]'::jsonb)
   FROM pg_default_acl d LEFT JOIN pg_namespace n ON n.oid=d.defaclnamespace
   WHERE d.defaclrole=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')
    OR EXISTS(SELECT 1 FROM aclexplode(d.defaclacl) a WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner'))),
 'schemas',(SELECT jsonb_agg(jsonb_build_array(n.nspname,pg_get_userbyid(n.nspowner),n.nspacl::text) ORDER BY n.nspname)
   FROM pg_namespace n WHERE n.nspname IN('submission_private','submission_api') OR n.nspowner=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')),
 'relations',(SELECT jsonb_agg(jsonb_build_array(n.nspname,c.relname,c.relkind,pg_get_userbyid(c.relowner),coalesce(c.relacl,acldefault('r',c.relowner))::text,
   c.relrowsecurity,c.relforcerowsecurity,
   (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,a.attgenerated,a.attacl::text,
     pg_get_expr(d.adbin,d.adrelid)) ORDER BY a.attnum) FROM pg_attribute a
    LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
   (SELECT jsonb_agg(pg_get_constraintdef(k.oid) ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=c.oid),
   (SELECT jsonb_agg(pg_get_indexdef(i.indexrelid) ORDER BY ci.relname) FROM pg_index i
    JOIN pg_class ci ON ci.oid=i.indexrelid WHERE i.indrelid=c.oid),
   (SELECT jsonb_agg(pg_get_triggerdef(t.oid) ORDER BY t.tgname) FROM pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal))
    ORDER BY n.nspname,c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE (n.nspname IN('submission_private','submission_api') OR c.relowner=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')) AND c.relkind IN('r','v','m','p')),
 'sequences',(SELECT jsonb_agg(jsonb_build_array(n.nspname,c.relname,pg_get_userbyid(c.relowner),coalesce(c.relacl,acldefault('s',c.relowner))::text,
  format_type(q.seqtypid,NULL),q.seqstart,q.seqincrement,q.seqmax,q.seqmin,q.seqcache,q.seqcycle) ORDER BY n.nspname,c.relname)
  FROM pg_sequence q JOIN pg_class c ON c.oid=q.seqrelid JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN('submission_private','submission_api') OR c.relowner=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner')),
 'functions',(SELECT jsonb_agg(jsonb_build_array(n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),
   pg_get_userbyid(p.proowner),p.proacl::text,pg_get_functiondef(p.oid)) ORDER BY n.nspname,p.proname,p.oid::regprocedure::text)
   FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE n.nspname IN('submission_private','submission_api') OR p.proowner=(SELECT oid FROM pg_roles WHERE rolname='board_submission_owner'))
 )::text,'UTF8'))
$$;
REVOKE ALL ON FUNCTION submission_private.catalog_hash() FROM PUBLIC;
CREATE FUNCTION submission_api.ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT definition_hash=submission_private.catalog_hash()
  FROM submission_private.catalog WHERE singleton),false)
 AND NOT EXISTS(SELECT 1 FROM (VALUES('board_public'),('board_staff'),('board_auth'),('board_verifier')) runtime(name)
  WHERE has_schema_privilege(runtime.name,'submission_private','USAGE,CREATE')
   OR pg_has_role(runtime.name,'board_submission_owner','MEMBER')
   OR pg_has_role(runtime.name,'board_migrator','MEMBER'))
$$;
REVOKE ALL ON FUNCTION submission_api.ready() FROM PUBLIC;
CREATE FUNCTION submission_api.feedback_profile() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE profile submission_private.feedback_profiles%ROWTYPE; categories jsonb;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 SELECT * INTO profile FROM submission_private.feedback_profiles WHERE enabled;
 IF NOT FOUND THEN RETURN NULL; END IF;
 SELECT jsonb_agg(jsonb_build_object('id',c.id,'label',c.label,'short_label',c.short_label) ORDER BY c.ordinal)
 INTO categories FROM submission_private.feedback_categories c WHERE c.profile_id=profile.id;
 IF categories IS NULL THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('id',profile.id,'subject_limit',profile.subject_limit,'message_limit',profile.message_limit,'categories',categories);
END $$;
CREATE FUNCTION submission_api.application_profile() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE profile submission_private.application_campaigns%ROWTYPE;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 SELECT * INTO profile FROM submission_private.application_campaigns WHERE enabled
  AND starts_at<=extract(epoch FROM statement_timestamp()) AND ends_at>extract(epoch FROM statement_timestamp());
 IF NOT FOUND THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('id',profile.id,'boards',profile.boards,'questions',profile.questions,
  'field_limit',profile.field_limit,'answer_limit',profile.answer_limit);
END $$;
CREATE FUNCTION submission_api.contest_profile(p_kind smallint) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE profile submission_private.contest_campaigns%ROWTYPE;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_kind IS NULL OR p_kind NOT IN(1,2) THEN RETURN NULL; END IF;
 SELECT * INTO profile FROM submission_private.contest_campaigns WHERE enabled AND kind=p_kind;
 IF NOT FOUND THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('id',profile.id,'kind',profile.kind,'boards',profile.boards,
  'submission_starts',profile.submission_starts,'submission_ends',profile.submission_ends,
  'voting_starts',profile.voting_starts,'voting_ends',profile.voting_ends,
  'capacity',profile.capacity,'current_time',floor(extract(epoch FROM statement_timestamp()))::bigint);
END $$;
REVOKE ALL ON FUNCTION submission_api.feedback_profile(),submission_api.application_profile(),submission_api.contest_profile(smallint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.ready(),submission_api.feedback_profile(),submission_api.application_profile(),submission_api.contest_profile(smallint) TO board_public,board_verifier;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_private,submission_api FROM board_submission_owner;
GRANT USAGE ON SCHEMA submission_api TO board_public,board_verifier;
SET LOCAL ROLE board_submission_owner;
SELECT set_config('board.submission_catalog_hash',encode(submission_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE submission_private.catalog SET definition_hash=decode(current_setting('board.submission_catalog_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission catalog was not sealed'; END IF;
END $$;
RESET ROLE;
