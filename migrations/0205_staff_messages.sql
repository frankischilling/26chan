-- Private operator messages for the source staff-message reader. No source
-- operator records are supplied, so the table starts empty.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction authority drifted before staff messages'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA restrictions,staff_identity TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;

CREATE TABLE restrictions.staff_messages (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 created_at bigint NOT NULL DEFAULT floor(extract(epoch FROM clock_timestamp()))::bigint
  CHECK(created_at BETWEEN 0 AND 253402300799),
 boards text[] NOT NULL DEFAULT '{}',
 content text NOT NULL CHECK(octet_length(content) BETWEEN 1 AND 16384),
 CHECK(cardinality(boards)<=128 AND array_position(boards,NULL) IS NULL
  AND (cardinality(boards)=0 OR (array_ndims(boards)=1 AND array_lower(boards,1)=1))),
 CHECK(array_to_string(boards,',') ~ '^([a-z0-9]{1,10}(,[a-z0-9]{1,10})*)?$')
);
CREATE INDEX staff_messages_created ON restrictions.staff_messages(created_at);
REVOKE ALL ON restrictions.staff_messages,restrictions.staff_messages_id_seq FROM PUBLIC;

CREATE FUNCTION restrictions.message_authority(p_session bytea,p_csrf bytea,p_idle integer)
RETURNS bigint LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; v_credential bytea;
BEGIN
 IF p_session IS NULL OR octet_length(p_session)<>32 OR p_csrf IS NULL OR octet_length(p_csrf)<>32
  OR p_idle IS NULL OR p_idle NOT BETWEEN 60 AND 3600 THEN
  RAISE EXCEPTION 'Staff message authority is unavailable' USING ERRCODE='P134A'; END IF;
 SELECT s.account_id,s.credential_id INTO account,v_credential
  FROM staff_identity.sessions s WHERE s.token_hash=p_session;
 IF NOT FOUND THEN RAISE EXCEPTION 'Staff message authority is unavailable' USING ERRCODE='P134A'; END IF;
 PERFORM a.id FROM staff_identity.accounts a WHERE a.id=account FOR SHARE NOWAIT;
 PERFORM s.token_hash FROM staff_identity.sessions s WHERE s.token_hash=p_session FOR SHARE NOWAIT;
 PERFORM c.id FROM staff_identity.credentials c WHERE c.id=v_credential FOR KEY SHARE NOWAIT;
 IF NOT EXISTS(
  SELECT 1 FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id
  JOIN staff_identity.credentials c ON c.id=s.credential_id AND c.account_id=a.id
  WHERE s.token_hash=p_session AND s.account_id=account AND s.credential_id=v_credential
   AND s.csrf_hash=p_csrf AND s.expires_at>clock_timestamp()
   AND s.last_activity_at>clock_timestamp()-make_interval(secs=>p_idle)
   AND a.revoked_at IS NULL AND a.role IN('janitor','moderator','manager','admin')) THEN
  RAISE EXCEPTION 'Staff message authority is unavailable' USING ERRCODE='P134A'; END IF;
 RETURN account;
END $$;

CREATE FUNCTION staff_identity.staff_message_page(
 p_session bytea,p_csrf bytea,p_idle integer,p_offset integer,p_seen bigint
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; moderator boolean; page jsonb; visible jsonb:='[]'; row jsonb;
 total integer; unread integer; next_offset integer; stamp bigint;
BEGIN
 IF restrictions.invoker() NOT IN('board_auth','board_migrator') THEN
  RAISE EXCEPTION 'Staff messages are unavailable' USING ERRCODE='42501'; END IF;
 IF p_offset IS NULL OR p_offset NOT BETWEEN 0 AND 10000 OR p_seen IS NULL OR p_seen<0 THEN
  RAISE EXCEPTION 'Invalid message page' USING ERRCODE='23514'; END IF;
 IF current_setting('transaction_isolation')<>'read committed'
  OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Staff message catalog is unavailable' USING ERRCODE='P134R'; END IF;
 account:=restrictions.message_authority(p_session,p_csrf,p_idle);
 SELECT a.role<>'janitor' INTO moderator FROM staff_identity.accounts a WHERE a.id=account;
 SELECT count(*) INTO total FROM (SELECT 1 FROM restrictions.staff_messages LIMIT 10001) bounded;
 IF total>10000 THEN RAISE EXCEPTION 'Staff message capacity is exhausted' USING ERRCODE='P134C'; END IF;
 stamp:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 -- Keep the source order: select 26 raw rows, apply board visibility, then
 -- remove the final visible row when the raw page had a sentinel.
 SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY m.id DESC),'[]') INTO page
  FROM (SELECT id,created_at,boards,content FROM restrictions.staff_messages
        ORDER BY id DESC OFFSET p_offset LIMIT 26) m;
 next_offset:=CASE WHEN jsonb_array_length(page)=26 THEN p_offset+25 ELSE 0 END;
 FOR row IN SELECT value FROM jsonb_array_elements(page) LOOP
  IF moderator OR jsonb_array_length(row->'boards')=0 OR EXISTS(
   SELECT 1 FROM jsonb_array_elements_text(row->'boards') b(slug)
    WHERE staff_identity.has_board_access(account,b.slug)) THEN
   visible:=visible||jsonb_build_array(row);
  END IF;
 END LOOP;
 IF next_offset<>0 AND jsonb_array_length(visible)>0 THEN
  visible:=visible-(jsonb_array_length(visible)-1);
 END IF;
 SELECT count(*) INTO unread FROM restrictions.staff_messages m
  WHERE m.created_at>p_seen AND (moderator OR cardinality(m.boards)=0 OR EXISTS(
   SELECT 1 FROM unnest(m.boards) b(slug) WHERE staff_identity.has_board_access(account,b.slug)));
 PERFORM restrictions.message_authority(p_session,p_csrf,p_idle);
 RETURN jsonb_build_object('messages',visible,'offset',p_offset,
  'previous_offset',greatest(0,p_offset-25),'next_offset',next_offset,
  'unread_count',unread,'read_at',stamp);
END $$;
REVOKE ALL ON FUNCTION restrictions.message_authority(bytea,bytea,integer),
 staff_identity.staff_message_page(bytea,bytea,integer,integer,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.staff_message_page(bytea,bytea,integer,integer,bigint) TO board_auth;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,staff_identity FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Staff messages changed restriction authority'; END IF; END $$;
RESET ROLE;
