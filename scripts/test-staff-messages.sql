-- Synthetic credentials and operator messages in the owned disposable database.
INSERT INTO staff_identity.accounts(username,role,allow_boards)
 VALUES('owned-message-janitor','janitor',ARRAY['a']),('owned-message-mod','moderator',ARRAY['a']);
INSERT INTO staff_identity.credentials(id,account_id,credential)
 SELECT convert_to(username,'UTF8'),id,'{}'::jsonb FROM staff_identity.accounts
 WHERE username IN('owned-message-janitor','owned-message-mod');
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
 SELECT decode(repeat(CASE WHEN role='janitor' THEN '11' ELSE '22' END,32),'hex'),decode(repeat('33',32),'hex'),
 id,convert_to(username,'UTF8') FROM staff_identity.accounts
 WHERE username IN('owned-message-janitor','owned-message-mod');
INSERT INTO restrictions.staff_messages(created_at,boards,content)
 SELECT 100+n,CASE WHEN n=1 THEN ARRAY['b'] ELSE ARRAY['a'] END,'Owned message '||n
 FROM generate_series(1,26) n;
DO $$ DECLARE runtime text; BEGIN
 FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF has_table_privilege(runtime,'restrictions.staff_messages','SELECT,INSERT,UPDATE,DELETE')
   OR has_any_column_privilege(runtime,'restrictions.staff_messages','SELECT,INSERT,UPDATE')
   OR has_sequence_privilege(runtime,'restrictions.staff_messages_id_seq','USAGE,SELECT,UPDATE')
   OR has_function_privilege(runtime,'restrictions.message_authority(bytea,bytea,integer)','EXECUTE')
   OR has_function_privilege(runtime,'staff_identity.staff_message_page(bytea,bytea,integer,integer,bigint)','EXECUTE') IS DISTINCT FROM (runtime='board_auth') THEN
   RAISE EXCEPTION 'Staff message role boundary changed for %',runtime; END IF;
 END LOOP;
END $$;
