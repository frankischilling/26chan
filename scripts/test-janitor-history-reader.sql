BEGIN;
DO $$ BEGIN
 IF has_function_privilege('board_public','staff_identity.janitor_request_history(bytea,bytea,integer,bigint,bigint)','EXECUTE')
 OR has_function_privilege('board_staff','staff_identity.janitor_request_history(bytea,bytea,integer,bigint,bigint)','EXECUTE') THEN
 RAISE EXCEPTION 'Private history reader is exposed'; END IF;
END $$;
SET LOCAL ROLE board_staff;
SELECT set_config('test.history_request',content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207007,9003,false,false)::text,true);
RESET ROLE;
UPDATE staff_identity.accounts SET allow_boards=ARRAY['rqown'] WHERE username='owned-request-reviewer';
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,upload_observation,template_id)
 SELECT CASE WHEN n=31 THEN 'rqother' ELSE 'rqown' END,207007,
 CASE WHEN n=32 THEN decode(repeat('fe',32),'hex') ELSE r.actor_hash END,
 n=31,false,statement_timestamp()-interval '1 day',NULL,'Owned history '||n,
 CASE WHEN n=31 THEN 'Hidden cross-board sentinel' WHEN n=32 THEN 'Hidden unrelated sentinel' ELSE '<script>Private note</script>' END,
 r.account_id,512,'absent',9003 FROM restrictions.requests r CROSS JOIN generate_series(1,32) n
 WHERE r.id=current_setting('test.history_request')::bigint;
UPDATE restrictions.capacity SET used_bytes=used_bytes+32*512 WHERE singleton;
SET LOCAL ROLE board_auth;
DO $$ DECLARE q jsonb; older jsonb; request bigint:=current_setting('test.history_request')::bigint; BEGIN
 q:=staff_identity.janitor_request_history(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,0);
 IF jsonb_array_length(q->'rows')<>25 OR (q->>'next_before')::bigint<=0
 OR q::text LIKE '%Hidden%' OR q::text LIKE '%actor_hash%' THEN RAISE EXCEPTION 'History page leaked or was not bounded'; END IF;
 older:=staff_identity.janitor_request_history(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,(q->>'next_before')::bigint);
 IF jsonb_array_length(older->'rows')<5 OR older::text LIKE '%Hidden%'
 OR EXISTS(SELECT 1 FROM jsonb_array_elements(q->'rows') a JOIN jsonb_array_elements(older->'rows') b ON a->>'id'=b->>'id') THEN
 RAISE EXCEPTION 'History cursor repeated or lost owned records'; END IF;
 BEGIN
  PERFORM staff_identity.janitor_request_history(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,request,0);
  RAISE EXCEPTION 'Janitor read private history'; EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 BEGIN
  PERFORM staff_identity.janitor_request_history(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,-1);
  RAISE EXCEPTION 'Negative cursor accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
RESET ROLE;
UPDATE staff_identity.accounts SET allow_boards=ARRAY['rqother'] WHERE username='owned-request-reviewer';
SET LOCAL ROLE board_auth;
DO $$ BEGIN
 BEGIN
  PERFORM staff_identity.janitor_request_history(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,current_setting('test.history_request')::bigint,0);
  RAISE EXCEPTION 'Changed source scope still read history'; EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
RESET ROLE;
UPDATE staff_identity.accounts SET allow_boards=ARRAY['rqown'] WHERE username='owned-request-reviewer';
SET LOCAL ROLE board_restriction_owner;
DELETE FROM restrictions.requests WHERE id=current_setting('test.history_request')::bigint;
SET LOCAL ROLE board_auth;
DO $$ BEGIN
 BEGIN
  PERFORM staff_identity.janitor_request_history(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,current_setting('test.history_request')::bigint,0);
  RAISE EXCEPTION 'Removed request still opened history'; EXCEPTION WHEN SQLSTATE 'P134N' THEN NULL; END;
END $$;
ROLLBACK;
