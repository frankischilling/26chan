DO $$ DECLARE page jsonb; BEGIN
 IF current_user<>'board_auth' THEN RAISE EXCEPTION 'Actual auth runtime is required'; END IF;
 page:=staff_identity.staff_message_page(decode(repeat('11',32),'hex'),decode(repeat('33',32),'hex'),600,0,110);
 -- All 26 rows are selected before visibility; the hidden raw sentinel still
 -- causes the source to pop its last visible row. Do not silently reorder it.
 IF jsonb_array_length(page->'messages')<>24 OR page->>'next_offset'<>'25'
  OR page->>'previous_offset'<>'0' OR page->>'unread_count'<>'16'
  OR page->'messages'->0->>'content'<>'Owned message 26'
  OR page->'messages'->23->>'content'<>'Owned message 3' THEN
  RAISE EXCEPTION 'Janitor message visibility or source pagination differs: %',page; END IF;
 page:=staff_identity.staff_message_page(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,0,0);
 IF jsonb_array_length(page->'messages')<>25 OR page->>'unread_count'<>'26' THEN
  RAISE EXCEPTION 'Moderator message visibility differs'; END IF;
 page:=staff_identity.staff_message_page(decode(repeat('11',32),'hex'),decode(repeat('33',32),'hex'),600,25,0);
 IF page->'messages'<>'[]'::jsonb OR page->>'next_offset'<>'0' OR page->>'previous_offset'<>'0' THEN
  RAISE EXCEPTION 'Hidden-only message page differs'; END IF;
 BEGIN
  PERFORM staff_identity.staff_message_page(decode(repeat('11',32),'hex'),decode(repeat('44',32),'hex'),600,0,0);
  RAISE EXCEPTION 'Wrong CSRF accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 BEGIN
  PERFORM staff_identity.staff_message_page(decode(repeat('44',32),'hex'),decode(repeat('33',32),'hex'),600,0,0);
  RAISE EXCEPTION 'Unknown session accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 BEGIN
  PERFORM staff_identity.staff_message_page(decode(repeat('11',32),'hex'),decode(repeat('33',32),'hex'),600,-1,0);
  RAISE EXCEPTION 'Negative offset accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  PERFORM 1 FROM restrictions.staff_messages;
  RAISE EXCEPTION 'Direct private message read accepted';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
