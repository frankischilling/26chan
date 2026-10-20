-- The classifier catalog seals board columns and their privileges too.
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Classifier history catalog is missing'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Classifier history drifted before Pass-only boards'; END IF; END $$;
RESET ROLE;
-- Preserve the source Pass-only board gate using consumed private authority.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before Pass post consumption'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass catalog drifted before post consumption'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before Pass post consumption'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_challenge_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Public challenge authority is unavailable'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.check_public_post_challenge()'::regprocedure
  AND proowner='board_anonymous_owner'::regrole AND prosecdef
  AND proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]
  AND encode(sha256(convert_to(prosrc,'UTF8')),'hex')='d17495d142ffecd0ca47921813bfc292728e2aee846f29fd4f8db87640f677c4') THEN
  RAISE EXCEPTION 'Post challenge consumer drifted before Pass-only boards'; END IF;
END $$;
-- Source global PASS_POST_ONLY=no; the active /vip/ override is yes.
ALTER TABLE content.boards ADD COLUMN pass_post_only boolean NOT NULL DEFAULT false;
UPDATE content.boards SET pass_post_only=true WHERE slug='vip';
GRANT SELECT(pass_post_only) ON content.boards TO board_anonymous_owner;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION content.check_public_post_challenge() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE credit_request text; invoker text; enabled boolean; pass_only boolean; raw text; proof jsonb; key text; session_hash bytea;
 peer bytea; receipt verification_private.receipts%ROWTYPE;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'posts' OR TG_OP<>'INSERT'
    OR TG_LEVEL<>'ROW' OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid post challenge trigger.' USING ERRCODE='23514'; END IF;
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 -- Staff writes retain their separate, independently authenticated authority.
 IF invoker IN('board_staff','board_migrator') THEN RETURN NEW; END IF;
 IF invoker<>'board_public' THEN RAISE EXCEPTION 'Post challenge authority is unavailable.' USING ERRCODE='42501'; END IF;
 SELECT b.captcha_enabled,b.pass_post_only INTO enabled,pass_only FROM content.boards b WHERE b.slug=NEW.board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 raw:=current_setting('board.challenge_context',true);
 PERFORM set_config('board.challenge_context','',true);
 credit_request:=current_setting('board.challenge_credit_request',true);
 PERFORM set_config('board.challenge_credit_request','',true);
 IF NOT enabled AND (raw IS NULL OR raw='') THEN RETURN NEW; END IF;
 IF enabled AND pass_only AND (raw IS NULL OR raw='') THEN
  RAISE EXCEPTION 'Only Pass users can post on this board.' USING ERRCODE='P0201'; END IF;
 IF raw IS NULL OR octet_length(raw)>1024 OR raw='' THEN
  RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166'; END IF;
 BEGIN proof:=raw::jsonb;
 EXCEPTION WHEN invalid_text_representation THEN
  RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END;
 IF jsonb_typeof(proof)<>'object' THEN
  RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(proof))<>4 THEN
  RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 FOREACH key IN ARRAY ARRAY['receipt','peer','session','context'] LOOP
  IF jsonb_typeof(proof->key) IS DISTINCT FROM 'string' OR octet_length(proof->>key)<>64
     OR (proof->>key) !~ '^[0-9a-f]{64}$' THEN
   RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 END LOOP;
 session_hash:=decode(proof->>'session','hex'); peer:=decode(proof->>'peer','hex');
 IF EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=decode(proof->>'receipt','hex')) THEN
  PERFORM * FROM verification_private.pass_take_receipt(decode(proof->>'receipt','hex'),peer,session_hash,decode(proof->>'context','hex'),
   CASE WHEN NEW.id=NEW.thread_id THEN 'thread' ELSE 'reply' END,NEW.board,
   CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END);
  INSERT INTO verification_private.post_handoffs(post_id,session_hash,peer_hash,may_grant_credit,registration_xid,credit_identity)
   VALUES(NEW.id,session_hash,peer,false,pg_current_xact_id(),NULL);
  INSERT INTO verification_private.pass_post_handoffs VALUES(NEW.id,session_hash,pg_current_xact_id());
  RETURN NEW;
 END IF;

 receipt:=verification_private.take_write_receipt_data(decode(proof->>'receipt','hex'),peer,session_hash,
  CASE WHEN NEW.id=NEW.thread_id THEN 'thread' ELSE 'reply' END,NEW.board,
  CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END,decode(proof->>'context','hex'));
 -- Source CAPTCHA-disabled mode itself sets captcha_bypass, even without a Pass.
 IF enabled AND pass_only THEN
  RAISE EXCEPTION 'Only Pass users can post on this board.' USING ERRCODE='P0201'; END IF;
 INSERT INTO verification_private.post_handoffs(post_id,session_hash,peer_hash,may_grant_credit,registration_xid,credit_identity)
 VALUES(NEW.id,session_hash,peer,receipt.may_grant_credit AND NOT receipt.suspicious AND receipt.unsolved_before<2
  AND coalesce(credit_request='eligible',false),pg_current_xact_id(),CASE WHEN receipt.from_credit THEN receipt.credit_identity ELSE NULL END);
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.check_public_post_challenge() FROM PUBLIC;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass post authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass post consumption changed transfer authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Pass post consumption changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_challenge_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass post consumption changed challenge authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass post consumption changed restriction authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_classifier_history_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog
 SET definition_hash=decode(current_setting('board.migration_classifier_history_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Pass-only boards changed classifier history authority'; END IF; END $$;
RESET ROLE;
