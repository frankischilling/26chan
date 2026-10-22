-- Preserve qualified source comment comparisons across sealed text-thread moves.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before comment comparison history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before comment comparison history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before comment comparison history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before comment comparison history'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before comment comparison history'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before comment comparison history'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;
-- PostgreSQL row-lock clauses require UPDATE on at least one column.
-- No runtime inherits this NOLOGIN owner; only sealed transfer functions use it.
GRANT SELECT,INSERT,UPDATE(post_id) ON post_secrets.comment_history TO board_thread_transfer_owner;
GRANT CREATE ON SCHEMA content,post_secrets,staff_identity TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
ALTER TABLE post_secrets.thread_transfer_sealed_rows ADD COLUMN original_comparison bytea
 CHECK(original_comparison IS NULL OR octet_length(original_comparison)=32);

CREATE FUNCTION staff_identity.seal_text_thread_transfer_comparisons(
 p_ticket bytea,p_session bytea,p_csrf bytea,p_fingerprint bytea,p_version smallint,
 p_old bigint[],p_new bigint[],p_comment text[],p_fp bytea[],p_search text[],p_original bytea[])
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE n integer;
BEGIN
 -- The existing issuer validates credentials, bounded source/map and every
 -- presentation before this wrapper stores a source-comparison assertion.
 PERFORM staff_identity.seal_text_thread_transfer(p_ticket,p_session,p_csrf,p_fingerprint,
  p_version,p_old,p_new,p_comment,p_fp,p_search);
 IF p_original IS NULL OR array_ndims(p_original) IS DISTINCT FROM 1
 OR array_lower(p_original,1) IS DISTINCT FROM 1
 OR cardinality(p_original) IS DISTINCT FROM cardinality(p_old)
 OR EXISTS(SELECT 1 FROM unnest(p_original) h WHERE h IS NULL OR octet_length(h)<>32) THEN
  RAISE EXCEPTION 'Invalid transfer comparison.' USING ERRCODE='P131I'; END IF;
 FOR n IN 1..cardinality(p_original) LOOP
  UPDATE post_secrets.thread_transfer_sealed_rows SET original_comparison=p_original[n]
   WHERE ticket_hash=p_ticket AND ordinal=n;
  IF NOT FOUND THEN RAISE EXCEPTION 'Transfer comparison map changed.' USING ERRCODE='P131C'; END IF;
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION staff_identity.seal_text_thread_transfer_comparisons(bytea,bytea,bytea,bytea,smallint,bigint[],bigint[],text[],bytea[],text[],bytea[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.seal_text_thread_transfer_comparisons(bytea,bytea,bytea,bytea,smallint,bigint[],bigint[],text[],bytea[],text[],bytea[]) TO board_auth;

-- Preserve the qualified transfer transaction and add comparison history only
-- after its existing source, authority and private-evidence checks succeed.
DO $migration$
DECLARE definition text; anchor text; insertion text;
BEGIN
 definition:=pg_get_functiondef('content.consume_text_thread_transfer(bytea)'::regprocedure);
 anchor:='    INSERT INTO post_secrets.thread_transfer_active';
 insertion:=$body$
    -- Lock known evidence before either destination insertion or source deletion.
    -- Retention takes only these rows; NOWAIT avoids an inverse lock wait.
    PERFORM h.post_id FROM post_secrets.comment_history h
      JOIN post_secrets.thread_transfer_map m ON m.source_post_id=h.post_id
      WHERE m.ticket_hash=p_ticket ORDER BY h.post_id FOR SHARE OF h NOWAIT;
    IF EXISTS(SELECT 1 FROM post_secrets.comment_history h
      JOIN post_secrets.thread_transfer_map m ON m.source_post_id=h.post_id
      JOIN post_secrets.thread_transfer_sealed_rows s ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal
      WHERE m.ticket_hash=p_ticket AND (h.board<>i.source_board OR h.projection_version<>1
        OR h.projection_hash IS DISTINCT FROM s.original_comparison)) THEN
      RAISE EXCEPTION 'Saved transfer comparison cannot be preserved.' USING ERRCODE='P131F'; END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.comment_history WHERE board=i.destination_board LIMIT 100001) bounded)
      +(SELECT count(*) FROM post_secrets.comment_history h JOIN post_secrets.thread_transfer_map m
        ON m.source_post_id=h.post_id WHERE m.ticket_hash=p_ticket)>100000 THEN
      RAISE EXCEPTION 'Transfer comparison capacity exhausted.' USING ERRCODE='P131L'; END IF;
$body$;
 IF (length(definition)-length(replace(definition,anchor,'')))/length(anchor)<>1 THEN
  RAISE EXCEPTION 'Unexpected transfer insertion boundary'; END IF;
 definition:=replace(definition,anchor,insertion||anchor);
 anchor:='    UPDATE content.threads SET deleted=true WHERE board=i.source_board AND id=i.source_thread;';
 insertion:=$body$
    -- The trusted formatter has sealed the rewritten source projection. A
    -- matching existing digest proves the original representation was known;
    -- absent historical evidence remains absent, never inferred from HTML.
    INSERT INTO post_secrets.comment_history(post_id,board,actor_hash,projection_hash,projection_version,request_at)
      SELECT m.destination_post_id,i.destination_board,h.actor_hash,
        sha256(convert_to('26chan:source-comment-projection:v1','UTF8')||decode('00','hex')||convert_to(s.comment,'UTF8')),
        h.projection_version,h.request_at
      FROM post_secrets.comment_history h JOIN post_secrets.thread_transfer_map m ON m.source_post_id=h.post_id
      JOIN post_secrets.thread_transfer_sealed_rows s ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal
      WHERE m.ticket_hash=p_ticket AND h.board=i.source_board AND h.projection_hash=s.original_comparison;
    IF EXISTS(
      WITH expected AS (
        SELECT m.destination_post_id AS post_id,i.destination_board AS board,h.actor_hash,
          sha256(convert_to('26chan:source-comment-projection:v1','UTF8')||decode('00','hex')||convert_to(s.comment,'UTF8')) AS projection_hash,
          h.projection_version,h.request_at
        FROM post_secrets.comment_history h JOIN post_secrets.thread_transfer_map m ON m.source_post_id=h.post_id
        JOIN post_secrets.thread_transfer_sealed_rows s ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal
        WHERE m.ticket_hash=p_ticket AND h.board=i.source_board
      ), actual AS (
        SELECT h.post_id,h.board,h.actor_hash,h.projection_hash,h.projection_version,h.request_at
        FROM post_secrets.comment_history h JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=h.post_id
        WHERE m.ticket_hash=p_ticket
      ) SELECT 1 FROM ((SELECT * FROM expected EXCEPT ALL SELECT * FROM actual)
        UNION ALL (SELECT * FROM actual EXCEPT ALL SELECT * FROM expected)) changed
    ) THEN RAISE EXCEPTION 'Transfer comparison verification failed.' USING ERRCODE='P131R'; END IF;
$body$;
 IF (length(definition)-length(replace(definition,anchor,'')))/length(anchor)<>1 THEN
  RAISE EXCEPTION 'Unexpected transfer retirement boundary'; END IF;
 EXECUTE replace(definition,anchor,insertion||anchor);
END $migration$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets,staff_identity FROM board_thread_transfer_owner;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_image_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_image_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_image_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_image_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer comparison history changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer comparison history changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer comparison history changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer comparison history changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer comparison history changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer comparison history changed challenge authority'; END IF; END $$;
RESET ROLE;
