DO $archive_age_transfer_preflight$
BEGIN
    IF (pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NULL)
        <> (pg_catalog.to_regprocedure('content.text_thread_transfer_ready()') IS NULL) THEN
        RAISE EXCEPTION 'Text transfer catalog is incomplete before archive age policy.';
    END IF;
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready for archive age policy.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $archive_age_transfer_preflight$;
-- Preserve the existing zero/positive encoding. -1 represents ENABLE_ARCHIVE
-- with ARCHIVE_MAX_AGE=0, without conflating unlimited storage with disabled UI.
DO $archive_age_snapshot_preflight$
BEGIN
    SET LOCAL ROLE board_report_snapshot_owner;
    PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
    IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Report snapshot catalog is not ready before archive age policy.';
    END IF;
    RESET ROLE;
END $archive_age_snapshot_preflight$;
ALTER TABLE content.boards DROP CONSTRAINT boards_archive_retention_seconds_check;
ALTER TABLE content.boards ADD CONSTRAINT boards_archive_retention_seconds_check
    CHECK (archive_retention_seconds BETWEEN -1 AND 2592000);
CREATE OR REPLACE VIEW content.visible_threads WITH (security_barrier = true) AS
    SELECT t.id,t.board,t.created_at,t.bumped_at,t.modified_at,t.reply_count,
        t.sticky,t.closed,t.deleted,t.archived_at,t.archive_expires_at,t.undead,
        t.permasage,t.permaage,t.http_modified_at,t.sticky_rank
    FROM content.threads t JOIN content.boards b ON b.slug=t.board
    WHERE (NOT b.staff_only OR current_user IN ('board_staff','board_migrator'))
      AND NOT t.deleted AND (t.archived_at IS NULL OR
        (b.archive_retention_seconds<>0 AND t.archive_expires_at>transaction_timestamp()));

GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
DO $archive_age_report_target$
DECLARE definition text;
BEGIN
    SELECT pg_get_functiondef('post_secrets.report_target(text,bigint,timestamptz)'::regprocedure)
        INTO definition;
    IF (length(definition)-length(replace(definition,'b.archive_retention_seconds>0','')))
        <>length('b.archive_retention_seconds>0') THEN
        RAISE EXCEPTION 'Report archive predicate differs from reviewed source.';
    END IF;
    SET LOCAL ROLE board_report_admission_owner;
    EXECUTE replace(definition,'b.archive_retention_seconds>0','b.archive_retention_seconds<>0');
    RESET ROLE;
END $archive_age_report_target$;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog
SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN
    IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Report snapshot catalog is not ready after archive age policy.';
    END IF;
END $$;
RESET ROLE;

-- The current board age applies to retained archives, including ones archived
-- before an operator changes that age. Keep the saved deadline used by every
-- visibility and admission path synchronized under the existing board lock.
CREATE FUNCTION content.refresh_archive_age_policy() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    -- A fixed transaction snapshot can miss a thread archived before this
    -- UPDATE acquired its board lock. Require a fresh statement snapshot.
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Archive age changes require Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Zero retains the legacy disabled-archive setting. It hides archives but
    -- does not erase their storage or replace deadlines with immediate expiry.
    IF NEW.archive_retention_seconds<>0 THEN
        UPDATE content.threads
        SET archive_expires_at=CASE WHEN NEW.archive_retention_seconds=-1 THEN 'infinity'::timestamptz
            ELSE archived_at+make_interval(secs=>NEW.archive_retention_seconds) END
        WHERE board=NEW.slug AND NOT deleted AND archived_at IS NOT NULL
          AND archive_expires_at IS DISTINCT FROM
              CASE WHEN NEW.archive_retention_seconds=-1 THEN 'infinity'::timestamptz
              ELSE archived_at+make_interval(secs=>NEW.archive_retention_seconds) END;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.refresh_archive_age_policy() FROM PUBLIC,
    board_public,board_staff,board_auth;
CREATE TRIGGER refresh_archive_age_policy AFTER UPDATE OF archive_retention_seconds
ON content.boards FOR EACH ROW
WHEN (OLD.archive_retention_seconds IS DISTINCT FROM NEW.archive_retention_seconds)
EXECUTE FUNCTION content.refresh_archive_age_policy();

DO $archive_age_transfer_reseal$
BEGIN
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        UPDATE post_secrets.thread_transfer_catalog
            SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready after archive age policy.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $archive_age_transfer_reseal$;

-- Reconcile live historical rows once. Deleted/erased content is never revived,
-- and disabled boards retain their stored deadlines until explicitly enabled.
DO $refresh_existing_archive_age$
DECLARE policy record;
BEGIN
    FOR policy IN SELECT slug,archive_retention_seconds FROM content.boards
        WHERE archive_retention_seconds<>0 ORDER BY slug FOR UPDATE
    LOOP
        UPDATE content.threads
        SET archive_expires_at=CASE WHEN policy.archive_retention_seconds=-1 THEN 'infinity'::timestamptz
            ELSE archived_at+make_interval(secs=>policy.archive_retention_seconds) END
        WHERE board=policy.slug AND NOT deleted AND archived_at IS NOT NULL
          AND archive_expires_at IS DISTINCT FROM
              CASE WHEN policy.archive_retention_seconds=-1 THEN 'infinity'::timestamptz
              ELSE archived_at+make_interval(secs=>policy.archive_retention_seconds) END;
    END LOOP;
END $refresh_existing_archive_age$;
