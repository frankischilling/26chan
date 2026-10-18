-- Request heuristics can withhold courtesy credits; they never validate a CAPTCHA.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before credit request policy'; END IF;
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
   AND encode(sha256(convert_to(prosrc,'UTF8')),'hex')='cde29aec41bd4ac80d7cd2374d292e33263c7fb2f9343f1113a2f792652c42e7'
   AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(proacl,acldefault('f',proowner))) a WHERE a.grantee<>proowner)) THEN
  RAISE EXCEPTION 'Post challenge consumer drifted before credit request policy'; END IF;
END $$;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION content.check_public_post_challenge() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE credit_request text; invoker text; enabled boolean; raw text; proof jsonb; key text; session_hash bytea;
 peer bytea; receipt verification_private.receipts%ROWTYPE;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'posts' OR TG_OP<>'INSERT'
    OR TG_LEVEL<>'ROW' OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid post challenge trigger.' USING ERRCODE='23514'; END IF;
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 -- Staff writes retain their separate, independently authenticated authority.
 IF invoker IN('board_staff','board_migrator') THEN RETURN NEW; END IF;
 IF invoker<>'board_public' THEN RAISE EXCEPTION 'Post challenge authority is unavailable.' USING ERRCODE='42501'; END IF;
 SELECT b.captcha_enabled INTO enabled FROM content.boards b WHERE b.slug=NEW.board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 raw:=current_setting('board.challenge_context',true);
 PERFORM set_config('board.challenge_context','',true);
 credit_request:=current_setting('board.challenge_credit_request',true);
 PERFORM set_config('board.challenge_credit_request','',true);
 IF NOT enabled THEN RETURN NEW; END IF;
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
 receipt:=verification_private.take_write_receipt_data(decode(proof->>'receipt','hex'),peer,session_hash,
  CASE WHEN NEW.id=NEW.thread_id THEN 'thread' ELSE 'reply' END,NEW.board,
  CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END,decode(proof->>'context','hex'));
 INSERT INTO verification_private.post_handoffs(post_id,session_hash,peer_hash,may_grant_credit,registration_xid,credit_identity)
 VALUES(NEW.id,session_hash,peer,receipt.may_grant_credit AND NOT receipt.suspicious AND receipt.unsolved_before<2
  AND coalesce(credit_request='eligible',false),pg_current_xact_id(),CASE WHEN receipt.from_credit THEN receipt.credit_identity ELSE NULL END);
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.check_public_post_challenge() FROM PUBLIC;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Credit request policy changed transfer authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_challenge_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Credit request policy changed challenge authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Credit request policy changed restriction authority'; END IF;
END $$;
RESET ROLE;
