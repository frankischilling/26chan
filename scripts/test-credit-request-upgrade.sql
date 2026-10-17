DO $$ BEGIN
 IF (SELECT rows FROM public.owned_credit_request_before) IS DISTINCT FROM public.owned_credit_request_rows() THEN
  RAISE EXCEPTION 'Credit request migration changed retained challenge/session state'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.check_public_post_challenge()'::regprocedure
   AND proowner='board_anonymous_owner'::regrole AND prosecdef
   AND proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]
   AND strpos(prosrc,'board.challenge_credit_request')>0
   AND strpos(prosrc,'coalesce(credit_request=''eligible'',false)')>0
   AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(proacl,acldefault('f',proowner))) a WHERE a.grantee<>proowner)) THEN
  RAISE EXCEPTION 'Credit request consumer lost its private restrictive contract'; END IF;
END $$;
SET ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Credit request challenge readiness failed'; END IF; END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Credit request transfer readiness failed'; END IF; END $$;
RESET ROLE;
