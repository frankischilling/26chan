-- imgboard.php:2719-2733 clears reports and their aggregate when a reply or
-- whole thread is removed. Only fresh whole-content deletion transitions are
-- changed here; public/staff file-only and archive retirement remain separate.
-- No historical sweep: already-deleted targets need separately reviewed,
-- bounded operator cleanup with board-first locking and explicit retention scope.
GRANT DELETE ON content.reports TO board_report_admission_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
CREATE FUNCTION post_secrets.delete_reports_for_deleted_target() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Normal deletion already owns board -> target. A raw UPDATE owns its
    -- target first: do not wait backwards against board-first admission.
    PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Report board is unavailable.' USING ERRCODE='23514';
    END IF;
    IF TG_TABLE_NAME='threads' THEN
        DELETE FROM content.reports r USING content.posts p
            WHERE r.board=NEW.board AND p.board=NEW.board
                AND r.post_id=p.id AND p.thread_id=NEW.id;
    ELSE
        DELETE FROM content.reports WHERE board=NEW.board AND post_id=NEW.id;
    END IF;
    -- Report FKs cascade membership, anonymous ownership and weight evidence.
    -- Membership's statement trigger retires only actually empty groups. This
    -- also covers historical reports without membership; no audit, upload or
    -- media rows are changed and no retained history seeds another lifetime.
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION post_secrets.delete_reports_for_deleted_target()
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
-- The migrator must bind the trigger on its content tables without inheriting
-- the private owner role. Remove this temporary EXECUTE grant after binding.
GRANT EXECUTE ON FUNCTION post_secrets.delete_reports_for_deleted_target() TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;

DROP TRIGGER retire_deleted_post_report_membership ON content.posts;
DROP TRIGGER retire_deleted_thread_report_membership ON content.threads;
CREATE TRIGGER retire_deleted_post_report_membership AFTER UPDATE OF deleted ON content.posts
    FOR EACH ROW WHEN (NOT OLD.deleted AND NEW.deleted)
    EXECUTE FUNCTION post_secrets.delete_reports_for_deleted_target();
CREATE TRIGGER retire_deleted_thread_report_membership AFTER UPDATE OF deleted ON content.threads
    FOR EACH ROW WHEN (NOT OLD.deleted AND NEW.deleted)
    EXECUTE FUNCTION post_secrets.delete_reports_for_deleted_target();
SET ROLE board_report_admission_owner;
REVOKE EXECUTE ON FUNCTION post_secrets.delete_reports_for_deleted_target() FROM board_migrator;
DROP FUNCTION post_secrets.retire_deleted_report_membership();
RESET ROLE;
