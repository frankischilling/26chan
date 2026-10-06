-- Source archive_thread clears pwd for the OP and every reply
-- (4chan-old/imgboard.php:1782-1800). Retire only new archive transitions:
-- installing this migration does not inspect or remove historical secrets.
GRANT SELECT(post_id),DELETE ON post_secrets.deletion TO board_posting_cooldown_owner;
-- PostgreSQL requires an UPDATE privilege for SELECT ... FOR UPDATE. No
-- function below changes thread identity; the role is NOLOGIN/NOBYPASSRLS.
GRANT UPDATE(id) ON content.threads TO board_posting_cooldown_owner;
-- 0087's board SELECT/UPDATE(slug) and 0090's post identity reads also let
-- 0037's caller-privilege deletion lock run under this restricted owner.
-- Existing board/thread/post/deletion RLS policies cover private boards;
-- no password_hash SELECT, runtime DELETE, or new RLS bypass is granted.

CREATE FUNCTION post_secrets.guard_archived_deletion_secret() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_post bigint; v_board text; v_thread bigint; v_old_board text; v_old_thread bigint;
    v_invoker text; v_parent record; v_found integer:=0; v_archived boolean:=false;
BEGIN
    -- Use the session/SET ROLE invoker, not this function's privileged owner.
    -- Ordinary staff's nested definer insertion retains the same actual role.
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Deletion authority is unavailable.' USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN v_post:=OLD.post_id; ELSE v_post:=NEW.post_id; END IF;
    SELECT p.board,p.thread_id INTO v_board,v_thread
        FROM content.posts p JOIN content.boards b ON b.slug=p.board
        WHERE p.id=v_post
            AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator'));
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Deletion authority is unavailable.' USING ERRCODE='23514';
    END IF;
    IF TG_OP='UPDATE' THEN
        SELECT p.board,p.thread_id INTO v_old_board,v_old_thread
            FROM content.posts p JOIN content.boards b ON b.slug=p.board
            WHERE p.id=OLD.post_id
                AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator'));
        IF NOT FOUND THEN
            RAISE EXCEPTION 'Deletion authority is unavailable.' USING ERRCODE='23514';
        END IF;
        -- UPDATE already owns a secret tuple. Reenter board locks without
        -- waiting to avoid a cycle with an archiver retiring that same tuple.
        -- Match 0037's sorted order if an operator reassigns a secret.
        PERFORM b.slug FROM content.boards b WHERE b.slug IN(v_old_board,v_board)
            ORDER BY b.slug FOR UPDATE OF b NOWAIT;
    ELSIF TG_OP='DELETE' THEN
        v_old_board:=v_board; v_old_thread:=v_thread;
        -- DELETE also owns its secret tuple before BEFORE triggers run.
        -- Never wait tuple -> board against archive's board -> secret order.
        PERFORM b.slug FROM content.boards b WHERE b.slug=v_board FOR UPDATE NOWAIT;
    ELSE
        v_old_board:=v_board; v_old_thread:=v_thread;
        PERFORM b.slug FROM content.boards b WHERE b.slug=v_board FOR UPDATE;
    END IF;
    -- Recheck visibility after board locks: a policy change may have committed
    -- while an INSERT waited. RR/Serializable fail on the changed board tuple.
    IF EXISTS(SELECT 1 FROM content.boards b WHERE b.slug IN(v_old_board,v_board)
        AND b.staff_only AND v_invoker NOT IN ('board_staff','board_migrator')) THEN
        RAISE EXCEPTION 'Deletion authority is unavailable.' USING ERRCODE='23514';
    END IF;
    -- Revocation is allowed for archived and soft-deleted content, including
    -- retirement's nested DELETE. It never acquires parent-thread locks.
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    -- Lock both parent threads in board/id order. Checking the OLD parent as
    -- well prevents moving a retained historical archived hash to active
    -- content. No deleted predicate: soft-deletion policy remains unchanged.
    -- RC follows a concurrent archive's committed tuple after waiting;
    -- Repeatable Read/Serializable fail with a serialization error instead
    -- of trusting the old snapshot. A pre-lock EXISTS check is insufficient.
    FOR v_parent IN
        SELECT t.archived_at FROM content.threads t
        WHERE (t.board=v_board AND t.id=v_thread)
            OR (t.board=v_old_board AND t.id=v_old_thread)
        ORDER BY t.board,t.id FOR UPDATE OF t
    LOOP
        v_found:=v_found+1;
        v_archived:=v_archived OR v_parent.archived_at IS NOT NULL;
    END LOOP;
    IF v_found<>(CASE WHEN v_board=v_old_board AND v_thread=v_old_thread THEN 1 ELSE 2 END)
        OR v_archived THEN
        RAISE EXCEPTION 'Deletion authority is unavailable.' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;

-- Alphabetically before deletion_authority_mutation: UPDATE/DELETE reenter
-- board locks NOWAIT first, with both boards sorted for UPDATE reassignment;
-- 0037 remains unchanged and caller-privileged.
CREATE TRIGGER deletion_archive_guard BEFORE INSERT OR UPDATE OR DELETE ON post_secrets.deletion
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_archived_deletion_secret();

CREATE FUNCTION post_secrets.retire_archived_deletion_secrets() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    -- An archive's fixed RR snapshot could miss a secret inserted before it
    -- obtained its locks. Archive transitions therefore require RC, including
    -- operator SQL. A failure rolls back the archive and all sibling triggers.
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Archive retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Normal writers already hold board -> thread. Direct operator SQL must
    -- not wait for a board while holding the updated thread: fail/retry with
    -- the board locked first instead. Once held, 0037 can safely reenter it.
    PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Archive board is unavailable.' USING ERRCODE='23514';
    END IF;
    DELETE FROM post_secrets.deletion d USING content.posts p
        WHERE d.post_id=p.id AND p.board=NEW.board AND p.thread_id=NEW.id;
    RETURN NEW;
END $$;

CREATE TRIGGER retire_archived_deletion_secrets AFTER UPDATE OF archived_at ON content.threads
    FOR EACH ROW WHEN (OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL)
    EXECUTE FUNCTION post_secrets.retire_archived_deletion_secrets();

REVOKE ALL ON FUNCTION post_secrets.guard_archived_deletion_secret(),
    post_secrets.retire_archived_deletion_secrets() FROM PUBLIC,board_public,board_staff,board_auth;
GRANT CREATE ON SCHEMA post_secrets TO board_posting_cooldown_owner;
ALTER FUNCTION post_secrets.guard_archived_deletion_secret() OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION post_secrets.retire_archived_deletion_secrets() OWNER TO board_posting_cooldown_owner;
REVOKE CREATE ON SCHEMA post_secrets FROM board_posting_cooldown_owner;
-- No restore path reconstructs a retired hash. The guard also rejects new or
-- rotated secrets on already archived rows, without deleting legacy rows.
-- This is logical password-authority retirement, not physical erasure; proof
-- derivatives, reports, audit, admission actions and media are unchanged.
