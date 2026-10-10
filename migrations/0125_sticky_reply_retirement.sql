-- imgboard.php:6484-6508 removes the oldest replies when a sticky+undead
-- thread exceeds STICKY_CAP. Pruned replies lose posting credentials as well
-- as public visibility. This runs only for fresh deletions made by that
-- bounded reply window; neither installation nor ordinary deletion sweeps
-- historical private state.
--
-- Reuse the NOLOGIN/NOBYPASSRLS archive-secret owner. It can already retire
-- deletion hashes; allow only post-ID selection and deletion of anonymous
-- membership proofs. Neither application runtime receives private DELETE.
GRANT SELECT(post_id), DELETE ON post_secrets.anonymous_posts TO board_posting_cooldown_owner;
GRANT SELECT(sticky,undead) ON content.threads TO board_posting_cooldown_owner;
GRANT SELECT(reply_limit) ON content.boards TO board_posting_cooldown_owner;
CREATE FUNCTION post_secrets.retire_pruned_reply_credentials() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_private boolean; v_limit integer; v_eligible boolean; v_newer bigint;
BEGIN
    -- The marker only identifies a source-window mutation. It grants no
    -- authority: the caller must already have marked this particular post
    -- deleted, and board/parent eligibility is checked independently below.
    IF current_setting('board.sticky_prune_thread',true) IS DISTINCT FROM NEW.thread_id::text THEN
        RETURN NEW;
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Sticky reply retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Sticky reply retirement is unavailable.' USING ERRCODE='23514';
    END IF;
    -- The ordinary poster already holds board -> thread -> post. An operator
    -- holding the post first must fail quickly, rather than wait backwards
    -- against a posting/deletion transaction. Private boards stay staff-only.
    PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE OF b NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Sticky reply board is unavailable.' USING ERRCODE='23514';
    END IF;
    SELECT b.staff_only,b.reply_limit INTO v_private,v_limit FROM content.boards b WHERE b.slug=NEW.board;
    IF v_private AND v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Sticky reply retirement is unavailable.' USING ERRCODE='23514';
    END IF;
    SELECT t.sticky AND t.undead AND NOT t.deleted AND t.archived_at IS NULL INTO v_eligible
        FROM content.threads t WHERE t.board=NEW.board AND t.id=NEW.thread_id;
    IF NOT coalesce(v_eligible,false) OR NEW.id=NEW.thread_id OR v_limit<=1 THEN
        RAISE EXCEPTION 'Sticky reply retirement is unavailable.' USING ERRCODE='23514';
    END IF;
    -- The transaction setting is caller-settable and therefore cannot grant
    -- permission to revoke a recent reply's proof. The old source retains the
    -- newest cap-1 existing IDs. At this AFTER trigger, only older victims
    -- have changed to deleted, so every legitimate victim still has at least
    -- cap-1 newer live replies. Gaps and other threads cannot satisfy it.
    SELECT count(*) INTO v_newer FROM content.posts p
        WHERE p.board=NEW.board AND p.thread_id=NEW.thread_id AND p.id<>NEW.thread_id
            AND p.id>NEW.id AND NOT p.deleted;
    IF v_newer < v_limit-1 THEN
        RAISE EXCEPTION 'Sticky reply retirement cannot remove a retained reply.' USING ERRCODE='23514';
    END IF;
    -- Delete the derived anonymous ownership first, then the hash it proves.
    -- Existing 0091 deletion guards reenter the same board with NOWAIT.
    DELETE FROM post_secrets.anonymous_posts WHERE post_id=NEW.id;
    DELETE FROM post_secrets.deletion WHERE post_id=NEW.id;
    RETURN NEW;
END $$;
-- Revoke the default PUBLIC permission while the migrator owns the function.
-- Its membership in the final owner role does not inherit that authority.
REVOKE ALL ON FUNCTION post_secrets.retire_pruned_reply_credentials()
    FROM PUBLIC,board_public,board_staff,board_auth;
CREATE TRIGGER retire_pruned_reply_credentials AFTER UPDATE OF deleted ON content.posts
    FOR EACH ROW WHEN (NOT OLD.deleted AND NEW.deleted AND NEW.id<>NEW.thread_id)
    EXECUTE FUNCTION post_secrets.retire_pruned_reply_credentials();
-- Bind the trigger before transferring ownership. The final ACL grants only
-- its NOLOGIN owner EXECUTE; runtime roles cannot invoke this definer directly.
GRANT CREATE ON SCHEMA post_secrets TO board_posting_cooldown_owner;
ALTER FUNCTION post_secrets.retire_pruned_reply_credentials() OWNER TO board_posting_cooldown_owner;
REVOKE CREATE ON SCHEMA post_secrets FROM board_posting_cooldown_owner;
