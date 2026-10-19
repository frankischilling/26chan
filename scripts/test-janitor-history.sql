BEGIN;
DO $$ BEGIN
 IF has_function_privilege('board_auth','restrictions.request_actor_history(bytea,bigint)','EXECUTE')
 OR has_function_privilege('board_staff','restrictions.request_actor_history(bytea,bigint)','EXECUTE')
 OR has_function_privilege('board_public','restrictions.request_actor_history(bytea,bigint)','EXECUTE') THEN
 RAISE EXCEPTION 'Actor history helper escaped the scoped queue'; END IF;
END $$;
SET LOCAL ROLE board_restriction_owner;
DO $$ DECLARE result jsonb; BEGIN
 result:=restrictions.request_actor_history(decode(repeat('ee',32),'hex'),2000000000);
 IF result->>'total'<>'0' THEN RAISE EXCEPTION 'Unknown actor inherited history'; END IF;
 BEGIN
  PERFORM restrictions.summarize_request_history((SELECT jsonb_agg('{}'::jsonb) FROM generate_series(1,10001)),2000000000);
  RAISE EXCEPTION 'Unbounded history was accepted';
 EXCEPTION WHEN SQLSTATE 'P134C' THEN NULL; END;
END $$;
UPDATE restrictions.templates SET public_reason='<script>Owned reason</script>' WHERE id=9001;
SET LOCAL ROLE board_auth;
DO $$ DECLARE q jsonb; BEGIN
 q:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,'rqown',0);
 IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(q->'templates') t WHERE t->>'id'='9001' AND t->>'reason'='<script>Owned reason</script>') THEN
 RAISE EXCEPTION 'Scoped public template reason is missing'; END IF;
 BEGIN
  PERFORM staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,'outside',0);
  RAISE EXCEPTION 'History queue ignored board scope';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
ROLLBACK;
