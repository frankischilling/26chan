-- Private automatic-ban exemptions preserve refusal responses while suppressing
-- ban insertion. No source lists or proxy ranges are imported.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before automatic-ban exemptions'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.record_content_admission(text,text,bigint,bigint,text,bigint,text,text,text,text)'::regprocedure
  AND proowner='board_admission_owner'::regrole AND prosecdef
  AND proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]
  AND encode(sha256(convert_to(prosrc,'UTF8')),'hex')='594948ff0e4c55b3496d5b7d3c218f18f0107e2f76474757f1dd0dbc0bd3f46c') THEN
  RAISE EXCEPTION 'Content admission recorder has drifted'; END IF;
END $$;
CREATE TABLE admission.automatic_ban_exemptions (
 network cidr PRIMARY KEY CHECK(family(network)=4 AND masklen(network) BETWEEN 1 AND 32
   AND host(network)<>'0.0.0.0'),
 active boolean NOT NULL DEFAULT true
);
CREATE INDEX automatic_ban_exemptions_active ON admission.automatic_ban_exemptions(network) WHERE active;
CREATE TRIGGER automatic_ban_exemptions_changed AFTER INSERT OR UPDATE OR DELETE OR TRUNCATE
 ON admission.automatic_ban_exemptions FOR EACH STATEMENT EXECUTE FUNCTION admission.changed();
GRANT SELECT ON admission.automatic_ban_exemptions TO board_admission_owner;
GRANT CREATE ON SCHEMA admission,content TO board_admission_owner;
SET LOCAL ROLE board_admission_owner;
CREATE FUNCTION admission.automatic_ban_exempt(p_peer inet) RETURNS boolean
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE ranges cidr[];
BEGIN
 IF p_peer IS NULL OR masklen(p_peer)<>(CASE family(p_peer) WHEN 4 THEN 32 ELSE 128 END) THEN
  RAISE EXCEPTION 'Invalid automatic-ban peer.' USING ERRCODE='23514'; END IF;
 -- The source whitelist is IPv4-only and has no forwarded-header input.
 IF family(p_peer)<>4 THEN RETURN false; END IF;
 SELECT ARRAY(SELECT r.network FROM admission.automatic_ban_exemptions r
   WHERE r.active ORDER BY r.network LIMIT 129) INTO ranges;
 IF cardinality(ranges)>128 THEN
  RAISE EXCEPTION 'Automatic-ban exemption policy is unavailable.' USING ERRCODE='55000'; END IF;
 RETURN EXISTS(SELECT 1 FROM unnest(ranges) AS r(network) WHERE p_peer <<= r.network);
END $$;
REVOKE ALL ON FUNCTION admission.automatic_ban_exempt(inet) FROM PUBLIC;
CREATE OR REPLACE FUNCTION content.record_content_admission(p_board text,p_peer text,p_revision bigint,
    p_rule bigint,p_kind text,p_parent bigint,p_name text,p_subject text,p_comment text,p_filename text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_revision bigint; r admission.rules%ROWTYPE; c admission.capacity%ROWTYPE;
    v_peer inet; v_days integer; v_now timestamptz:=clock_timestamp(); v_reason text;
BEGIN
    SELECT revision INTO v_revision FROM admission.policy WHERE singleton FOR SHARE;
    IF NOT FOUND OR p_revision IS NULL OR v_revision<>p_revision THEN
        RAISE EXCEPTION 'Content admission policy changed.' USING ERRCODE='55000';
    END IF;
    IF p_kind IS NULL OR p_kind NOT IN ('filename','autosage','log','reject','quiet')
        OR p_peer IS NULL OR p_parent IS NULL OR p_parent<0 OR p_name IS NULL OR octet_length(p_name)>2048
        OR p_subject IS NULL OR octet_length(p_subject)>2048 OR p_comment IS NULL OR octet_length(p_comment)>131072
        OR p_filename IS NULL OR octet_length(p_filename)>1024
        OR NOT EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only) THEN
        RAISE EXCEPTION 'Invalid content admission effect.' USING ERRCODE='23514';
    END IF;
    v_peer:=p_peer::inet;
    IF masklen(v_peer)<>(CASE family(v_peer) WHEN 4 THEN 32 ELSE 128 END) THEN
        RAISE EXCEPTION 'Invalid content admission peer.' USING ERRCODE='23514';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('26chan:admission:'||host(v_peer),0));
    IF p_kind='filename' AND p_rule IS NULL AND left(p_filename,3)='php'
        AND octet_length(p_filename)>=5 AND strpos(p_filename,'.')=0
        AND position(decode('0a','hex') IN substring(convert_to(p_filename,'UTF8') FROM 4 FOR 2))=0 THEN
        v_days:=14; v_reason:='filename';
    ELSE
        SELECT * INTO r FROM admission.rules WHERE id=p_rule AND active AND (board='' OR board=p_board);
        IF NOT FOUND OR (p_parent>0 AND (r.ops_only OR r.autosage)) OR
            NOT ((p_kind='autosage' AND r.autosage) OR
                 (p_kind='log' AND NOT r.autosage AND r.log) OR
                 (p_kind='reject' AND NOT r.autosage AND NOT r.log AND NOT r.quiet) OR
                 (p_kind='quiet' AND NOT r.autosage AND NOT r.log AND r.quiet)) THEN
            RAISE EXCEPTION 'Content admission effect is not permitted.' USING ERRCODE='23514';
        END IF;
        v_days:=CASE WHEN p_kind IN ('reject','quiet') THEN r.ban_days ELSE 0 END;
        v_reason:='content';
    END IF;
    -- Capacity has a separate row from policy. Readers never upgrade a shared
    -- policy lock while another board is evaluating the same generation.
    SELECT * INTO c FROM admission.capacity WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Content admission capacity is unavailable.' USING ERRCODE='55000'; END IF;
    IF p_rule IS NOT NULL THEN
        IF NOT EXISTS(SELECT 1 FROM admission.hits WHERE rule_id=p_rule AND peer=v_peer)
            AND (SELECT count(*) FROM (SELECT 1 FROM admission.hits LIMIT c.hit_limit) bounded)>=c.hit_limit THEN
            RAISE EXCEPTION 'Content admission hit capacity exhausted.' USING ERRCODE='53300';
        END IF;
        INSERT INTO admission.hits(rule_id,peer,board,last_hit,count) VALUES(p_rule,v_peer,p_board,v_now,1)
            ON CONFLICT(rule_id,peer) DO UPDATE SET count=admission.hits.count+1,last_hit=v_now,board=p_board
            WHERE admission.hits.last_hit<=v_now-interval '1 hour';
    END IF;
    IF p_kind='log' THEN
        IF (SELECT count(*) FROM (SELECT 1 FROM admission.logs LIMIT c.log_limit) bounded)>=c.log_limit THEN
            RAISE EXCEPTION 'Content admission log capacity exhausted.' USING ERRCODE='53300';
        END IF;
        INSERT INTO admission.logs(rule_id,board,parent,peer,name,subject,comment,filename)
        VALUES(p_rule,p_board,p_parent,v_peer,p_name,p_subject,p_comment,p_filename);
    END IF;
    IF v_days<>0 AND NOT admission.automatic_ban_exempt(v_peer) THEN
        IF NOT EXISTS(SELECT 1 FROM admission.bans WHERE peer=v_peer)
            AND (SELECT count(*) FROM (SELECT 1 FROM admission.bans LIMIT c.ban_limit) bounded)>=c.ban_limit THEN
            RAISE EXCEPTION 'Content admission ban capacity exhausted.' USING ERRCODE='53300';
        END IF;
        INSERT INTO admission.bans(peer,board,rule_id,reason,expires_at)
        VALUES(v_peer,p_board,p_rule,v_reason,CASE WHEN v_days=-1 THEN NULL
            ELSE date_trunc('day',v_now AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' + make_interval(days=>v_days) END)
        ON CONFLICT(peer) DO UPDATE SET board=p_board,rule_id=p_rule,reason=v_reason,
            expires_at=EXCLUDED.expires_at,created_at=v_now,active=true;
    END IF;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA admission,content FROM board_admission_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Automatic-ban exemptions changed transfer authority'; END IF;
END $$;
RESET ROLE;
