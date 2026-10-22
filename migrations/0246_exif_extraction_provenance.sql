-- Apply final opening-comment rules with locked, operator-owned board policy.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before EXIF provenance'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before EXIF provenance'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before EXIF provenance'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before EXIF provenance'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before EXIF provenance'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before EXIF provenance'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;
-- Existing assets remain unknown; never backfill an empty extraction.
ALTER TABLE media.assets
 ADD COLUMN exif_text text,
 ADD COLUMN exif_mode smallint,
 ADD CONSTRAINT exif_manifest CHECK (
   (exif_text IS NULL AND exif_mode IS NULL)
   OR (exif_text IS NOT NULL AND exif_mode IS NOT NULL AND exif_mode IN (1,2)
       AND output_format='jpeg' AND still_profile IS NOT NULL AND still_profile='still-v6'
       AND (exif_mode<>1 OR exif_text='')
       AND octet_length(exif_text)<=16384
       AND cardinality(string_to_array(exif_text,E'\n'))<=256));
CREATE FUNCTION media.guard_exif_manifest() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_OP='UPDATE' THEN
  IF (NEW.exif_text,NEW.exif_mode) IS DISTINCT FROM (OLD.exif_text,OLD.exif_mode) THEN
   RAISE EXCEPTION 'EXIF extraction provenance is immutable.' USING ERRCODE='42501';
  END IF;
 ELSIF NEW.exif_mode IS NOT NULL THEN
  PERFORM 1 FROM media.jobs WHERE id=NEW.job_id FOR UPDATE;
  IF NEW.state<>'pending' OR NOT EXISTS (
    SELECT 1 FROM media.jobs j JOIN media.image_context c ON c.job_id=j.id
    WHERE j.id=NEW.job_id AND j.state='processing' AND j.input_kind='image-v1'
      AND j.lease_token=NEW.lease_token AND j.expires_at>clock_timestamp()
      AND j.input_bytes=NEW.source_input_bytes
      AND ((c.board='p' AND NEW.exif_mode=2) OR (c.board='test' AND NEW.exif_mode=1))
  ) THEN
   RAISE EXCEPTION 'EXIF extraction requires its context-bound processing lease.' USING ERRCODE='42501';
  END IF;
 END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_exif_manifest() FROM PUBLIC;
CREATE TRIGGER media_exif_manifest_immutable BEFORE INSERT OR UPDATE ON media.assets
 FOR EACH ROW EXECUTE FUNCTION media.guard_exif_manifest();
GRANT SELECT(exif_text,exif_mode) ON media.assets TO board_attachment_owner;
-- A bounded transaction-local reservation binds the comparison timestamp to
-- the later attachment. Deferred checking prevents committed unused tickets.
CREATE TABLE post_secrets.exif_preparations (
 job_id text PRIMARY KEY REFERENCES media.jobs(id) ON DELETE CASCADE,
 transaction_id xid8 NOT NULL,
 board text NOT NULL,
 parent bigint NOT NULL CHECK(parent>=0),
 tim bigint NOT NULL UNIQUE CHECK(tim BETWEEN 1 AND 9007199254740991)
);
REVOKE ALL ON post_secrets.exif_preparations FROM PUBLIC;
GRANT SELECT,INSERT,DELETE ON post_secrets.exif_preparations TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE FUNCTION content.prepare_attachment_exif(p_job text,p_capability text,p_board text,p_parent bigint,p_authorized boolean,p_badged boolean)
RETURNS TABLE(exif_text text,tim bigint) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE value text; format text; number bigint; prior record;
BEGIN
 -- This locks and verifies the exact board, reply target, job, capability,
 -- current approval, image policy and authenticated upload context.
 PERFORM content.attachment_upload_filter_metadata(p_job,p_capability,p_board,p_parent,p_authorized,p_badged);
 SELECT a.exif_text,a.output_format INTO value,format FROM media.assets a
  WHERE a.job_id=p_job AND a.state='approved' ORDER BY a.id LIMIT 1;
 IF NOT FOUND THEN RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002'; END IF;
 IF p_board NOT IN ('p','test') OR format<>'jpeg' THEN value:=''; END IF;
 IF value IS NOT NULL AND value NOT IN ('','0') THEN
  SELECT * INTO prior FROM post_secrets.exif_preparations WHERE job_id=p_job;
  IF FOUND THEN
   IF prior.transaction_id<>pg_current_xact_id() OR prior.board<>p_board OR prior.parent<>p_parent THEN
    RAISE EXCEPTION 'EXIF preparation is unavailable.' USING ERRCODE='P0002';
   END IF;
   number:=prior.tim;
  ELSE
   number:=content.next_media_number();
   INSERT INTO post_secrets.exif_preparations(job_id,transaction_id,board,parent,tim)
    VALUES(p_job,pg_current_xact_id(),p_board,p_parent,number);
  END IF;
 END IF;
 RETURN QUERY SELECT value,number;
END $$;
REVOKE ALL ON FUNCTION content.prepare_attachment_exif(text,text,text,bigint,boolean,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.prepare_attachment_exif(text,text,text,bigint,boolean,boolean) TO board_public,board_staff;
CREATE FUNCTION content.exif_post_prepared(p_board text,p_post bigint,p_thread bigint) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 RETURN EXISTS(SELECT 1 FROM post_secrets.exif_preparations e WHERE e.transaction_id=pg_current_xact_id()
   AND e.board=p_board AND e.parent=CASE WHEN p_post=p_thread THEN 0 ELSE p_thread END);
END $$;
REVOKE ALL ON FUNCTION content.exif_post_prepared(text,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.exif_post_prepared(text,bigint,bigint) TO board_public,board_staff,board_attachment_owner;
CREATE FUNCTION content.discard_attachment_exif() RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 DELETE FROM post_secrets.exif_preparations WHERE transaction_id=pg_current_xact_id();
END $$;
REVOKE ALL ON FUNCTION content.discard_attachment_exif() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.discard_attachment_exif() TO board_public,board_staff;
CREATE FUNCTION content.consume_attachment_exif() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE prepared record; p record; value text;
BEGIN
 SELECT * INTO prepared FROM post_secrets.exif_preparations WHERE job_id=NEW.job_id;
 SELECT board,thread_id INTO p FROM content.posts WHERE id=NEW.post_id;
 SELECT exif_text INTO value FROM media.assets WHERE id=NEW.asset_id AND job_id=NEW.job_id AND state='approved';
 IF prepared.job_id IS NOT NULL THEN
  IF prepared.transaction_id<>pg_current_xact_id() OR prepared.board IS DISTINCT FROM p.board
    OR prepared.parent IS DISTINCT FROM (CASE WHEN NEW.post_id=p.thread_id THEN 0 ELSE p.thread_id END)
    OR value IS NULL OR value IN ('','0') THEN
   RAISE EXCEPTION 'EXIF preparation does not match this post.' USING ERRCODE='42501';
  END IF;
  NEW.tim:=prepared.tim;
  DELETE FROM post_secrets.exif_preparations WHERE job_id=NEW.job_id;
 ELSIF p.board IN ('p','test') AND value IS NOT NULL AND value NOT IN ('','0') THEN
  RAISE EXCEPTION 'EXIF preparation is required.' USING ERRCODE='42501';
 END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.consume_attachment_exif() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.consume_attachment_exif() TO board_migrator;
CREATE FUNCTION content.require_consumed_exif() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.exif_preparations WHERE job_id=NEW.job_id AND transaction_id=NEW.transaction_id) THEN
  RAISE EXCEPTION 'Unused EXIF preparation cannot be committed.' USING ERRCODE='42501';
 END IF;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION content.require_consumed_exif() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.require_consumed_exif() TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
CREATE TRIGGER post_media_exif_preparation BEFORE INSERT ON content.post_media
 FOR EACH ROW EXECUTE FUNCTION content.consume_attachment_exif();
CREATE CONSTRAINT TRIGGER exif_preparation_consumed AFTER INSERT ON post_secrets.exif_preparations
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION content.require_consumed_exif();
ALTER TABLE content.posts DROP CONSTRAINT posts_wordfilter_payload_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_wordfilter_payload_check CHECK (
    wordfilter_payload IS NULL OR (
        octet_length(wordfilter_payload) BETWEEN 11 AND CASE WHEN staff_authorized_limits THEN 524288 ELSE 131072 END
        AND substring(wordfilter_payload FROM 1 FOR 4) IN (
            decode(CASE WHEN staff_authorized_limits THEN '57463032' ELSE '57463031' END,'hex'),
            decode(CASE WHEN staff_authorized_limits THEN '57463034' ELSE '57463033' END,'hex'),
            decode(CASE WHEN staff_authorized_limits THEN '57463036' ELSE '57463035' END,'hex'))));

ALTER TABLE post_secrets.staff_post_intents DROP CONSTRAINT staff_post_intents_wordfilter_payload_check;
ALTER TABLE post_secrets.staff_post_intents ADD CONSTRAINT staff_post_intents_wordfilter_payload_check CHECK (
    wordfilter_payload IS NULL OR (
        octet_length(wordfilter_payload) BETWEEN 11 AND CASE WHEN authorized_limits THEN 524288 ELSE 131072 END
        AND substring(wordfilter_payload FROM 1 FOR 4) IN (
            decode(CASE WHEN authorized_limits THEN '57463032' ELSE '57463031' END,'hex'),
            decode(CASE WHEN authorized_limits THEN '57463034' ELSE '57463033' END,'hex'),
            decode(CASE WHEN authorized_limits THEN '57463036' ELSE '57463035' END,'hex'))));

ALTER TABLE content.posts ADD CONSTRAINT posts_exif_read_budget CHECK (
 substring(wordfilter_payload FROM 1 FOR 4) NOT IN (decode('57463035','hex'),decode('57463036','hex'))
 OR octet_length(comment)+octet_length(wordfilter_payload)+coalesce(octet_length(wordfilter_search),0)
   <= CASE WHEN staff_authorized_limits THEN 3145728 ELSE 64000 END);
ALTER TABLE content.posts DROP CONSTRAINT posts_generated_payload_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_generated_payload_check CHECK (
 (coalesce(substring(wordfilter_payload FROM 1 FOR 4) IN (decode('57463033','hex'),decode('57463034','hex'),decode('57463035','hex'),decode('57463036','hex')),false))
 = (comment_format BETWEEN 488 AND 495 OR comment_format BETWEEN 504 AND 511)
 AND (NOT (comment_format BETWEEN 488 AND 495 OR comment_format BETWEEN 504 AND 511)
   OR dice_result IS NOT NULL OR fortune_text IS NOT NULL
   OR substring(wordfilter_payload FROM 1 FOR 4) IN (decode('57463035','hex'),decode('57463036','hex'))));
GRANT CREATE ON SCHEMA staff_identity TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;
DO $exif_issuers$
DECLARE signature regprocedure; body text; needle text:= 'decode(CASE WHEN v_authorized THEN ''57463034'' ELSE ''57463033'' END,''hex'')';
BEGIN
 FOR signature IN SELECT p.oid::regprocedure FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE n.nspname='staff_identity' AND ((p.proname='issue_limited_post_authority' AND p.pronargs=16)
    OR (p.proname='issue_ordinary_post_authority' AND p.pronargs=19)) LOOP
  body:=pg_get_functiondef(signature);
  IF (length(body)-length(replace(body,needle,'')))/length(needle)<>1 THEN
   RAISE EXCEPTION 'Staff payload validation changed before EXIF support'; END IF;
  EXECUTE replace(body,needle,needle||',decode(CASE WHEN v_authorized THEN ''57463036'' ELSE ''57463035'' END,''hex'')');
 END LOOP;
END $exif_issuers$;
RESET ROLE;
REVOKE CREATE ON SCHEMA staff_identity FROM board_staff_post_owner;
CREATE OR REPLACE FUNCTION content.stamp_wordfilter_payload() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean; payload text; maximum integer; generated boolean;
BEGIN
    SELECT b.word_filter_enabled INTO enabled FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    NEW.wordfilter_payload:=NULL;
    NEW.wordfilter_search:=NULL;
    payload:=current_setting('board.wordfilter_payload',true);
    generated:=coalesce(left(payload,8) IN ('57463033','57463034','57463035','57463036'),false);
    IF enabled OR generated THEN
        maximum:=CASE WHEN NEW.staff_authorized_limits THEN 1048576 ELSE 262144 END;
        IF payload IS NULL OR length(payload) NOT BETWEEN 22 AND maximum OR payload !~ '^[0-9a-f]+$'
            OR length(payload)%2<>0 OR (generated AND NEW.dice_result IS NULL AND NEW.fortune_text IS NULL AND left(payload,8) NOT IN ('57463035','57463036')) THEN
            RAISE EXCEPTION 'Wordfilter result is unavailable.' USING ERRCODE='23514';
        END IF;
        IF left(payload,8) IN ('57463035','57463036') AND NOT content.exif_post_prepared(NEW.board,NEW.id,NEW.thread_id) THEN
            RAISE EXCEPTION 'EXIF preparation is unavailable.' USING ERRCODE='23514';
        END IF;
        NEW.wordfilter_payload:=decode(payload,'hex');
        NEW.wordfilter_search:=current_setting('board.wordfilter_search',true);
    ELSE
        IF current_user='board_staff' AND coalesce(payload,'')<>'' THEN
            RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
        END IF;
        PERFORM set_config('board.wordfilter_payload','',true);
        PERFORM set_config('board.wordfilter_search','',true);
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.stamp_wordfilter_payload() FROM PUBLIC;

CREATE OR REPLACE FUNCTION content.stamp_comment_format() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_format smallint;
BEGIN
    SELECT (232 + b.comment_spoiler_cleanup::integer
        + 2 * b.comment_code_spacing::integer
        + 4 * b.comment_sjis_spacing::integer
        + 16 * (b.op_markup AND (NEW.id=NEW.thread_id
            OR COALESCE(current_setting('board.source_op_reply', true)='true', false)))::integer)::smallint
        INTO v_format FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE = '23503';
    END IF;
    IF substring(NEW.wordfilter_payload FROM 1 FOR 4) IN (decode('57463033','hex'),decode('57463034','hex'),decode('57463035','hex'),decode('57463036','hex')) THEN
        v_format := v_format + 256;
    END IF;
    NEW.comment_format := v_format;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.stamp_comment_format() FROM PUBLIC;

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
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'EXIF provenance changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'EXIF provenance changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'EXIF provenance changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'EXIF provenance changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'EXIF provenance changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'EXIF provenance changed challenge authority'; END IF; END $$;
RESET ROLE;
