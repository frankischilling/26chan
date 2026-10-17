-- Private storage for file-origin restrictions. No runtime ban issuer is added.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before file provenance'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before file provenance'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA restrictions TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
ALTER TABLE restrictions.bans
 ADD COLUMN origin text NOT NULL DEFAULT 'staff' CHECK(origin IN('staff','file','dmca')),
 ADD COLUMN file_rule_id bigint CHECK(file_rule_id>0),
 ADD COLUMN file_digest bytea CHECK(octet_length(file_digest)=16),
 ALTER COLUMN post_id DROP NOT NULL,
 ALTER COLUMN account_id DROP NOT NULL;
-- Historical rule identity survives operator rule deletion. It confers no
-- current authority; a future issuer must revalidate the live rule separately.
ALTER TABLE restrictions.bans ADD CONSTRAINT restriction_origin_provenance CHECK (
 (origin='staff' AND post_id IS NOT NULL AND account_id IS NOT NULL
    AND file_rule_id IS NULL AND file_digest IS NULL)
 OR (origin IN('file','dmca') AND post_id IS NULL AND account_id IS NULL
    AND file_rule_id IS NOT NULL AND file_digest IS NOT NULL)
);
-- Staff warnings retain the existing equality contract. Source automatic
-- warnings may end at an earlier local midnight, before the database start.
ALTER TABLE restrictions.bans DROP CONSTRAINT bans_check;
ALTER TABLE restrictions.bans ADD CONSTRAINT restriction_duration CHECK (
 (warning AND ends_at IS NOT NULL
    AND ((origin='staff' AND ends_at=starts_at) OR (origin<>'staff' AND ends_at<=starts_at)))
 OR (NOT warning AND (ends_at IS NULL OR ends_at>starts_at))
);
CREATE OR REPLACE FUNCTION restrictions.posting_decision(p_actor bytea,p_token bytea,p_board text,p_request bigint)
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
    'template_id',0,'admin',CASE WHEN origin='staff' THEN 'Staff' ELSE 'Auto-ban' END,
    'reason',CASE WHEN origin='staff' THEN '' ELSE public_reason||'<>'||private_reason END) ORDER BY position,id),'[]'::jsonb)
 INTO v_count,v_rows FROM bounded;
 IF v_count>1000 THEN RAISE EXCEPTION 'Restriction context exceeds its bound' USING ERRCODE='P134C'; END IF;
 -- Pass authority is not installed. Automatic provenance comes only from the
 -- private restriction row, never from a caller-supplied request flag.
 RETURN QUERY SELECT d.disposition,d.expired FROM restrictions.source_decision(v_rows,p_board,p_request,false,false,v_verified) d;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions FROM board_restriction_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'File restriction provenance changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'File restriction provenance changed restriction authority'; END IF; END $$;
RESET ROLE;
