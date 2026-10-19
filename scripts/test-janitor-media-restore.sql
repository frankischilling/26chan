DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true
 OR (SELECT used_bytes FROM restrictions.request_media_capacity)<>2062
 OR NOT EXISTS(SELECT 1 FROM restrictions.request_media m JOIN restrictions.requests r ON r.id=m.request_id
   WHERE r.post_id=207006 AND m.body=convert_to('full','UTF8') AND m.thumbnail=convert_to('thumb','UTF8'))
 OR NOT EXISTS(SELECT 1 FROM restrictions.request_ban_thumbnails t JOIN restrictions.bans b ON b.id=t.ban_id
   WHERE t.post_id=207005 AND b.post_id=207005 AND t.thumbnail=convert_to('thumb','UTF8'))
 OR EXISTS(SELECT 1 FROM media.approved_post_assets a JOIN content.post_media m ON m.tim=a.tim WHERE m.post_id IN(207005,207006))
 OR has_table_privilege('board_public','restrictions.request_media','SELECT')
 OR has_table_privilege('board_auth','restrictions.request_media','SELECT')
 OR has_table_privilege('board_media_read','restrictions.request_media','SELECT') THEN
 RAISE EXCEPTION 'Populated private custody restore differs'; END IF;
END $$;
SELECT id AS pending_request FROM restrictions.requests WHERE post_id=207006 \gset
SET ROLE board_auth;
SELECT encode(body,'escape')='full' AS readable FROM staff_identity.janitor_request_media(
 decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,:pending_request,false) \gset
\if :readable
\else
\quit 1
\endif
RESET ROLE;
