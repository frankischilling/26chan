\set ON_ERROR_STOP on
BEGIN;
SET LOCAL ROLE board_migrator;
DO $exemption$
DECLARE board_name text:='rx'||substr(replace(gen_random_uuid()::text,'-',''),1,8);
 actor bytea:=decode(repeat('e3',32),'hex'); epoch_a bytea:=decode(repeat('f4',32),'hex'); epoch_b bytea:=decode(repeat('a5',32),'hex');
 stamp timestamptz:=clock_timestamp(); restriction bigint; result boolean;
BEGIN
 INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES(board_name,'Owned restriction history','Synthetic',2000,100,100,100,10);
 EXECUTE 'SET LOCAL ROLE board_restriction_owner';
 PERFORM restrictions.gate(true);
 INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,active,retired_at,retirement)
 VALUES(board_name,1,actor,false,true,stamp-interval '29 days',stamp-interval '29 days','Owned warning','',1,512,false,stamp,'warning-viewed') RETURNING id INTO restriction;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 IF content.report_restriction_history_clear(actor,stamp) IS DISTINCT FROM false THEN
  RAISE EXCEPTION 'Retired warning disappeared from the thirty-day window'; END IF;
 EXECUTE 'SET LOCAL ROLE board_restriction_owner';
 UPDATE restrictions.bans SET starts_at=stamp-interval '30 days',ends_at=stamp-interval '30 days' WHERE id=restriction;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 IF content.report_restriction_history_clear(actor,stamp) IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction history lost the strict thirty-day edge'; END IF;
 -- Existing retired events age out without permanently disabling first adoption.
 UPDATE verification_private.policy SET report_history_epoch=NULL,report_history_epoch_tainted=false WHERE singleton;
 PERFORM verification_private.observe_report_epoch(epoch_a);
 IF (SELECT report_history_epoch_tainted FROM verification_private.policy WHERE singleton) THEN
  RAISE EXCEPTION 'Retired history tainted otherwise initial continuity'; END IF;
 -- Unmappable active warnings and indefinite bans cannot gain exemption.
 EXECUTE 'SET LOCAL ROLE board_restriction_owner';
 UPDATE restrictions.bans SET active=true,retired_at=NULL,retirement=NULL WHERE id=restriction;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 UPDATE verification_private.policy SET report_history_epoch=NULL,report_history_epoch_tainted=false WHERE singleton;
 PERFORM verification_private.observe_report_epoch(epoch_a);
 IF NOT (SELECT report_history_epoch_tainted FROM verification_private.policy WHERE singleton) THEN
  RAISE EXCEPTION 'Unknown active warning continuity was trusted'; END IF;
 EXECUTE 'SET LOCAL ROLE board_restriction_owner';
 UPDATE restrictions.bans SET warning=false,ends_at=NULL WHERE id=restriction;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 UPDATE verification_private.policy SET report_history_epoch=NULL,report_history_epoch_tainted=false WHERE singleton;
 PERFORM verification_private.observe_report_epoch(epoch_a);
 IF NOT (SELECT report_history_epoch_tainted FROM verification_private.policy WHERE singleton) THEN
  RAISE EXCEPTION 'Unknown indefinite-ban continuity was trusted'; END IF;
 EXECUTE 'SET LOCAL ROLE board_restriction_owner';
 UPDATE restrictions.bans SET active=false,retired_at=stamp,retirement='unban' WHERE id=restriction;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 UPDATE verification_private.policy SET report_history_epoch=epoch_a,report_history_epoch_tainted=false WHERE singleton;
 PERFORM verification_private.observe_report_epoch(epoch_b);
 PERFORM verification_private.observe_report_epoch(epoch_a);
 IF NOT (SELECT report_history_epoch_tainted FROM verification_private.policy WHERE singleton) THEN
  RAISE EXCEPTION 'Returning to an old epoch cleared a tainted continuity flag'; END IF;
 UPDATE verification_private.policy SET report_history_epoch_tainted=false WHERE singleton;
 PERFORM verification_private.observe_report_epoch(NULL);
 PERFORM verification_private.observe_report_epoch(epoch_a);
 IF NOT (SELECT report_history_epoch_tainted FROM verification_private.policy WHERE singleton) THEN
  RAISE EXCEPTION 'Missing metadata did not persistently taint continuity'; END IF;
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_public';
 BEGIN PERFORM content.report_restriction_history_clear(actor,stamp);
  RAISE EXCEPTION 'Public role could read private warning history'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM content.report_active_restrictions_exist();
  RAISE EXCEPTION 'Public role could inspect active restrictions'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $exemption$;
RESET ROLE;
ROLLBACK;
