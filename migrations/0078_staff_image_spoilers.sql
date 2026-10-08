-- The source stores image-spoiler state in a reserved subject prefix, including
-- on posts without a file. Keep it separate from the user-visible subject.
ALTER TABLE content.posts ADD COLUMN image_spoiler boolean NOT NULL DEFAULT false;
UPDATE content.posts p SET image_spoiler=m.spoiler
    FROM content.post_media m WHERE m.post_id=p.id;
GRANT SELECT(image_spoiler),UPDATE(image_spoiler) ON content.posts TO board_attachment_owner;
GRANT UPDATE(spoiler) ON content.post_media TO board_attachment_owner;

CREATE FUNCTION content.sync_image_spoiler() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    UPDATE content.posts SET image_spoiler=NEW.spoiler
        WHERE id=NEW.post_id AND image_spoiler IS DISTINCT FROM NEW.spoiler;
    RETURN NEW;
END $$;

-- The caller holds current staff authority and appends an audit in the same
-- transaction. No public credential can invoke this setter or write either
-- representation directly. Repeated requests leave timestamps/audits alone.
CREATE FUNCTION content.set_post_image_spoiler(p_board text,p_post bigint,p_spoiler boolean)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_enabled boolean; v_thread bigint; v_old boolean;
BEGIN
    IF p_spoiler IS NULL OR p_post IS NULL OR p_post<=0 THEN
        RAISE EXCEPTION 'Invalid spoiler action.' USING ERRCODE='22023';
    END IF;
    SELECT b.comment_spoiler_cleanup INTO v_enabled FROM content.boards b
        WHERE b.slug=p_board FOR UPDATE;
    IF v_enabled IS NULL THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002';
    ELSIF NOT v_enabled THEN
        RAISE EXCEPTION 'Spoilers are disabled on this board.' USING ERRCODE='22023';
    END IF;
    SELECT p.thread_id INTO v_thread FROM content.posts p
        WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002';
    END IF;
    PERFORM t.id FROM content.threads t JOIN content.visible_threads v ON v.id=t.id AND v.board=t.board
        WHERE t.id=v_thread AND t.board=p_board FOR UPDATE OF t;
    IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM content.visible_threads v
        WHERE v.id=v_thread AND v.board=p_board
          AND (v.archived_at IS NULL OR v.archive_expires_at>clock_timestamp())) THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002';
    END IF;
    SELECT p.image_spoiler INTO v_old FROM content.posts p
        WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF v_old=p_spoiler THEN RETURN false; END IF;
    UPDATE content.posts SET image_spoiler=p_spoiler WHERE id=p_post;
    UPDATE content.post_media SET spoiler=p_spoiler WHERE post_id=p_post;
    UPDATE content.threads SET modified_at=clock_timestamp() WHERE id=v_thread;
    RETURN true;
END $$;

REVOKE ALL ON FUNCTION content.sync_image_spoiler(),content.set_post_image_spoiler(text,bigint,boolean) FROM PUBLIC;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
ALTER FUNCTION content.sync_image_spoiler() OWNER TO board_attachment_owner;
ALTER FUNCTION content.set_post_image_spoiler(text,bigint,boolean) OWNER TO board_attachment_owner;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
GRANT EXECUTE ON FUNCTION content.set_post_image_spoiler(text,bigint,boolean) TO board_staff;
GRANT EXECUTE ON FUNCTION content.sync_image_spoiler() TO board_migrator;
RESET ROLE;
CREATE TRIGGER sync_image_spoiler AFTER INSERT OR UPDATE OF spoiler ON content.post_media
    FOR EACH ROW EXECUTE FUNCTION content.sync_image_spoiler();
SET LOCAL ROLE board_attachment_owner;
REVOKE EXECUTE ON FUNCTION content.sync_image_spoiler() FROM board_migrator;
RESET ROLE;

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK(action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler'));
