-- Source delete_post distinguishes a staff member's own password or host
-- from an administrative deletion (imgboard.php:2490-2497,2545-2594).
-- No historical ownership is inferred and no new raw peer data is stored.
DO $self_deletion_transfer_preflight$
BEGIN
    SET LOCAL ROLE board_thread_transfer_owner;
    PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
    IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Text transfer catalog is not ready before staff self deletion.';
    END IF;
    RESET ROLE;
END $self_deletion_transfer_preflight$;

GRANT CREATE ON SCHEMA content TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;
CREATE FUNCTION content.staff_post_deletion_hash(p_board text,p_post bigint) RETURNS text
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE invoker text; result text;
BEGIN
    invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Staff ownership evidence is unavailable.' USING ERRCODE='42501';
    END IF;
    SELECT d.password_hash INTO result
    FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id
    JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
    JOIN content.boards b ON b.slug=p.board
    WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted AND NOT t.deleted
        AND t.archived_at IS NULL AND NOT b.staff_only;
    RETURN result;
END $$;
REVOKE ALL ON FUNCTION content.staff_post_deletion_hash(text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.staff_post_deletion_hash(text,bigint) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_staff_post_owner;

GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
SET LOCAL ROLE board_posting_cooldown_owner;
CREATE FUNCTION content.staff_post_peer_matches(p_board text,p_post bigint,p_actor bytea) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE invoker text;
BEGIN
    invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Staff ownership evidence is unavailable.' USING ERRCODE='42501';
    END IF;
    RETURN coalesce(octet_length(p_actor)=32,false) AND EXISTS (
        SELECT 1 FROM post_secrets.posting_history h
        JOIN content.posts p ON p.id=h.post_id AND p.board=h.board
        JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
        WHERE h.board=p_board AND h.post_id=p_post AND h.thread_id=p.thread_id
            AND h.actor_hash=p_actor AND NOT p.deleted AND NOT t.deleted
            AND t.archived_at IS NULL);
END $$;
REVOKE ALL ON FUNCTION content.staff_post_peer_matches(text,bigint,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.staff_post_peer_matches(text,bigint,bytea) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check CHECK (
    action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler',
        'undead','unundead','thread-options','force-archive','reporter-clear','report-group-clear',
        'text-thread-transfer','self-remove-post','self-remove-file','self-remove-thread'));

SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash()
    WHERE singleton;
DO $$ BEGIN
    IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Text transfer catalog is not ready after staff self deletion.';
    END IF;
END $$;
RESET ROLE;
