BEGIN;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.templates SET source_rule='global1' WHERE id=9001;
UPDATE restrictions.templates SET source_rule='owned-shortcut' WHERE id=9002;
SET LOCAL ROLE board_auth;
DO $$ DECLARE queue jsonb; BEGIN
 queue:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,NULL,0);
 IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(queue->'templates') t WHERE t->>'id'='9001' AND t->'no_warn'='true'::jsonb)
 OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(queue->'templates') t WHERE t->>'id'='9002' AND t->'no_warn'='false'::jsonb) THEN
 RAISE EXCEPTION 'Source rule warning-shortcut metadata differs'; END IF;
END $$;
ROLLBACK;
