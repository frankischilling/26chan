-- Stored April 2024 presentation values are not Pass purchase years.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before optional event authority'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM event_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted before score scopes'; END IF;
END $$;
RESET ROLE;
-- No historical identities or event selections are imported.
ALTER TABLE content.posts ADD COLUMN event_perk smallint CHECK(event_perk BETWEEN 1 AND 231);
ALTER TABLE content.posts ADD CONSTRAINT posts_event_perk_erased CHECK(NOT content_erased OR event_perk IS NULL);
GRANT SELECT(event_perk),INSERT(event_perk) ON content.posts TO board_thread_transfer_owner;
-- Finite released text for search; this function reads no table or identity.
GRANT CREATE ON SCHEMA content TO board_event_owner;
SET LOCAL ROLE board_event_owner;
CREATE FUNCTION content.event_perk_text(p_id smallint) RETURNS text
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE SET search_path=pg_catalog,pg_temp AS $$
 SELECT (ARRAY['😃','🙁','👌','🐭','🐹','🐰','🐶','🐺','🦊','🐵','🐸','🙈','🙉','🙊','🐯','🦁','🦓','🦒','🐴','🐮','🐷','🐻','🐼','🐲','🦄','🐱','😸','😹','😺','😻','😼','😽','😾','😿','🙀','🐅','🐆','🐘','🦏','🐂','🐃','🐄','🐎','🦌','🐐','🐏','🐑','🐖','🐗','🐪','🐫','🦍','🐉','🦖','🦕','🐈','🐀','🐁','🐇','🐒','🐕','🐩','🐨','🐿','🦔','🦇','🐍','🦅','🦉','🦆','🐓','🐔','🦃','🕊','🐣','🐤','🐥','🐦','🐧','🐋','🐳','🐬','🦈','🐟','🐠','🐡','🐙','🦑','🦐','🦀','🐚','🐌','🐢','🦎','🐊','🏇','🎠','♘','♞','🐽','🐾','👣','🐀','🐃','🐅','🐇','🐉','🐍','🐎','🐐','🐒','🐓','🐕','🐖','🧀','🥚','🍳','🥞','🍠','🍞','🥐','🥖','🥨','🍔','🍕','🍝','🍟','🍤','🌭','🌮','🌯','🍛','🥙','🥘','🥗','🥪','🥫','🥓','🍖','🍗','🥩','🥢','🥡','🥟','🍚','🍜','🍲','🥠','🍘','🍙','🍣','🍥','🍱','🍡','🍢','🍇','🍈','🍉','🍊','🍋','🍌','🍍','🍎','🍏','🍐','🍑','🍒','🍓','🥝','🥥','🥦','🍄','🍅','🍆','🌶','🥑','🥕','🥒','🥔','🥜','🍰','🎂','🥧','🍨','🍦','🍩','🍪','🍿','🍮','🍯','🍧','🍫','🍬','🍭','🍺','🍻','🍷','🍸','🍹','🍶','🥂','🥃','🍾','☕','🍵','🥛','🍼','🥤','🍴','🍽','🥣','🥄','✔️','❌','❤️','💔','💢','💯','♨️','❤️','💙','💜','💛','🖤','💚','😂','🐷','🐶','🐱','🌈','🐸']::text[])[p_id]
$$;
REVOKE ALL ON FUNCTION content.event_perk_text(smallint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_perk_text(smallint) TO board_public,board_staff,board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_event_owner;

GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION content.stamp_event_perk() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE invoker text; requested text; run_id bigint;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'posts' OR TG_NAME<>'zz_event_perk'
 OR TG_OP NOT IN('INSERT','UPDATE') OR TG_WHEN<>'BEFORE' OR TG_LEVEL<>'ROW' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid event perk context' USING ERRCODE='23514'; END IF;
 -- The existing erasure trigger runs first; erased rows retain no decoration.
 IF NEW.content_erased THEN NEW.event_perk:=NULL; RETURN NEW; END IF;
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 -- Migrator imports and authenticated staff transfers preserve finite saved
 -- selections, exactly as they preserve published Pass-year presentation.
 IF invoker IN('board_staff','board_migrator') THEN RETURN NEW; END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.event_perk IS DISTINCT FROM OLD.event_perk THEN
   RAISE EXCEPTION 'Stored event perk cannot be changed' USING ERRCODE='42501'; END IF;
  RETURN NEW;
 END IF;
 NEW.event_perk:=NULL;
 requested:=current_setting('board.event_perk',true);
 PERFORM set_config('board.event_perk','',true);
 IF invoker<>'board_public' OR current_setting('board.event_scores',true) IS DISTINCT FROM 'true'
 OR requested IS NULL OR requested='' THEN RETURN NEW; END IF;
 IF requested !~ '^[1-9][0-9]{0,2}$' OR requested::integer NOT BETWEEN 1 AND 231 THEN
  RAISE EXCEPTION 'Invalid event perk choice' USING ERRCODE='23514'; END IF;
 IF content.events_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 -- Earlier BEFORE triggers consumed the posting proof. Hold the run through
 -- insertion so closure cannot interleave with a persisted selection.
 SELECT id INTO run_id FROM event_private.runs WHERE kind=3 AND enabled
  AND starts_at<=extract(epoch FROM clock_timestamp()) AND ends_at>extract(epoch FROM clock_timestamp()) FOR UPDATE;
 IF NOT FOUND THEN RETURN NEW; END IF;
 IF NOT EXISTS(SELECT 1 FROM event_private.runs WHERE id=run_id AND enabled
  AND ends_at>extract(epoch FROM clock_timestamp())) THEN RETURN NEW; END IF;
 NEW.event_perk:=requested::smallint;
 RETURN NEW;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
CREATE TRIGGER zz_event_perk BEFORE INSERT OR UPDATE ON content.posts FOR EACH ROW EXECUTE FUNCTION content.stamp_event_perk();
SET LOCAL ROLE board_anonymous_owner;
REVOKE ALL ON FUNCTION content.stamp_event_perk() FROM PUBLIC;
RESET ROLE;
-- Retain the new finite field through the existing authenticated transfer.
-- All predecessor catalogs were verified above; exact fragments refuse drift.
GRANT CREATE ON SCHEMA content,post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$
DECLARE definition text; changed text;
BEGIN
 definition:=pg_get_functiondef('post_secrets.transfer_post_hash(bigint)'::regprocedure);
 changed:=replace(definition,'p.staff_notices,p.since4pass,','p.staff_notices,p.since4pass,p.event_perk,');
 IF changed=definition THEN RAISE EXCEPTION 'Transfer hash lacks source identity fields'; END IF;
 EXECUTE changed;
 definition:=pg_get_functiondef('content.consume_text_thread_transfer(bytea)'::regprocedure);
 changed:=replace(definition,'staff_notices,since4pass)','staff_notices,since4pass,event_perk)');
 changed:=replace(changed,'p.staff_notices,p.since4pass','p.staff_notices,p.since4pass,p.event_perk');
 changed:=replace(changed,'d.staff_notices,d.since4pass,','d.staff_notices,d.since4pass,d.event_perk,');
 IF changed=definition OR strpos(changed,'staff_notices,since4pass,event_perk)')=0
 OR strpos(changed,'d.staff_notices,d.since4pass,d.event_perk,')=0 THEN
  RAISE EXCEPTION 'Transfer copy lacks source identity fields'; END IF;
 EXECUTE changed;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_thread_transfer_owner;
SET LOCAL ROLE board_event_owner;
SELECT set_config('board.migration_score_event_hash',encode(event_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE event_private.catalog SET definition_hash=decode(current_setting('board.migration_score_event_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_image_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_image_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_image_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_image_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN IF content.events_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Perk presentation migration changed event authority'; END IF; END $$;
RESET ROLE;
