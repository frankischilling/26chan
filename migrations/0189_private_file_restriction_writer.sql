-- Private persistence only. The future file-effect caller must qualify the live
-- rule, exemption, attachment and response before invoking this operation.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before private file writer'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before private file writer'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA restrictions TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE FUNCTION restrictions.file_expiry(p_started timestamptz,p_days integer,p_timezone text)
RETURNS timestamptz LANGUAGE plpgsql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' AS $$
BEGIN
 IF p_started IS NULL OR NOT isfinite(p_started) OR p_days IS NULL OR p_days NOT BETWEEN -1 AND 3650
    OR p_timezone IS NULL OR octet_length(p_timezone) NOT BETWEEN 1 AND 64
    OR NOT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name=p_timezone) THEN
  RAISE EXCEPTION 'Invalid automatic restriction duration.' USING ERRCODE='23514'; END IF;
 IF p_days=-1 THEN RETURN NULL; END IF;
 -- PHP adds seconds before formatting a local date, then appends midnight.
 -- Do not use calendar-day interval addition across daylight-saving changes.
 RETURN date_trunc('day',(p_started+make_interval(secs=>p_days::double precision*86400)) AT TIME ZONE p_timezone)
    AT TIME ZONE p_timezone;
END $$;
REVOKE ALL ON FUNCTION restrictions.file_expiry(timestamptz,integer,text) FROM PUBLIC;

CREATE FUNCTION restrictions.record_file_ban(
 p_actor bytea,p_token bytea,p_board text,p_rule bigint,p_digest bytea,
 p_days integer,p_dmca boolean,p_public text,p_private text,p_timezone text
) RETURNS bigint LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' AS $$
DECLARE v_now timestamptz; v_end timestamptz; v_identity uuid; v_id bigint; v_charge bigint;
 c restrictions.capacity%ROWTYPE;
BEGIN
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR (p_token IS NOT NULL AND octet_length(p_token)<>32)
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_rule IS NULL OR p_rule<=0
    OR p_digest IS NULL OR octet_length(p_digest)<>16 OR p_dmca IS NULL
    OR (p_dmca AND p_days IS DISTINCT FROM 3)
    OR p_public IS NULL OR octet_length(p_public) NOT BETWEEN 1 AND 16384
    OR p_private IS NULL OR octet_length(p_private)>32768 THEN
  RAISE EXCEPTION 'Invalid private file restriction.' USING ERRCODE='23514'; END IF;
 -- Invoke from a fresh effect transaction after posting rollback, before any
 -- board/media locks. A caller must never upgrade its posting gate in place.
 PERFORM content.lock_posting_actor(p_actor,false);
 PERFORM restrictions.gate(true);
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR SHARE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'File restriction board is unavailable.' USING ERRCODE='P134N'; END IF;
 v_now:=date_trunc('second',clock_timestamp());
 v_end:=restrictions.file_expiry(v_now,p_days,p_timezone);
 SELECT a.automatic_identity INTO v_identity
  FROM post_secrets.lookup_automatic_identity(p_token,floor(extract(epoch FROM v_now))::bigint) a;
 -- Existing trusted equality is retained even for a creation-second/idle-reset
 -- session. Unknown, expired and not-yet-persisted sessions yield no identity.
 SELECT * INTO STRICT c FROM restrictions.capacity WHERE singleton;
 v_charge:=512+octet_length(p_public)::bigint+octet_length(p_private)::bigint;
 IF c.used_bytes>c.byte_limit-v_charge
    OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans LIMIT c.row_limit) bounded)>=c.row_limit THEN
  RAISE EXCEPTION 'Restriction capacity is exhausted.' USING ERRCODE='P134C'; END IF;
 INSERT INTO restrictions.bans(board,actor_hash,automatic_identity,global,warning,starts_at,ends_at,
  public_reason,private_reason,charged_bytes,origin,file_rule_id,file_digest)
 VALUES(p_board,p_actor,v_identity,true,v_end IS NOT NULL AND v_end<=v_now,v_now,v_end,
  p_public,p_private,v_charge,CASE WHEN p_dmca THEN 'dmca' ELSE 'file' END,p_rule,p_digest)
 RETURNING id INTO v_id;
 UPDATE restrictions.capacity SET used_bytes=used_bytes+v_charge WHERE singleton;
 INSERT INTO restrictions.audit(ban_id,action)
 VALUES(v_id,CASE WHEN v_end IS NOT NULL AND v_end<=v_now THEN 'warning' ELSE 'ban' END);
 RETURN v_id;
END $$;
REVOKE ALL ON FUNCTION restrictions.record_file_ban(bytea,bytea,text,bigint,bytea,integer,boolean,text,text,text) FROM PUBLIC;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions FROM board_restriction_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Private file writer changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Private file writer changed restriction authority'; END IF; END $$;
RESET ROLE;
