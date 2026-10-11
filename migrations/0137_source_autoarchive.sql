DO $autoarchive_transfer_preflight$
BEGIN
    IF (pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NULL)
        <> (pg_catalog.to_regprocedure('content.text_thread_transfer_ready()') IS NULL) THEN
        RAISE EXCEPTION 'Text transfer catalog is incomplete before autoarchive policy.';
    END IF;
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready for autoarchive policy.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $autoarchive_transfer_preflight$;
-- Source AUTOARCHIVE_CAP: global default 0; /jp/ and /vt/ use 1500.
-- Add policy without archiving existing threads during migration. Only a
-- subsequently accepted reply can trigger the source threshold transition.
ALTER TABLE content.boards ADD COLUMN autoarchive_cap integer NOT NULL DEFAULT 0
    CHECK (autoarchive_cap BETWEEN 0 AND 1500);
ALTER TABLE content.threads DROP CONSTRAINT threads_reply_count_check;
ALTER TABLE content.threads ADD CONSTRAINT threads_reply_count_check CHECK(reply_count BETWEEN 0 AND 1500);
UPDATE content.boards SET autoarchive_cap=1500 WHERE slug IN ('jp','vt');
-- Ordinary autoarchive threads use the larger of this cap and reply_limit.
-- Preserve reply_limit itself, including the separate sticky/undead window.
-- Runtime roles keep read access but gain no board-policy UPDATE grants.
DO $autoarchive_transfer_reseal$
BEGIN
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        UPDATE post_secrets.thread_transfer_catalog
            SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready after autoarchive policy.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $autoarchive_transfer_reseal$;
