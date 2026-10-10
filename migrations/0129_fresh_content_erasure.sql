-- Fresh whole-deletion erasure. Existing soft-deleted history is untouched.
-- Structural rows preserve attachment consumption/revocation; this is not SQL
-- hard-delete parity or a policy for historical data, audit or media metadata.
-- Drawing annotations are post payload. Their independent moderation snapshots
-- and global poll configuration/ballots have no per-post erasure relationship.
ALTER TABLE content.posts ADD COLUMN content_erased boolean NOT NULL DEFAULT false;
ALTER TABLE content.threads ADD COLUMN content_erased boolean NOT NULL DEFAULT false;
ALTER TABLE content.posts ADD CONSTRAINT posts_erased_payload CHECK (
    NOT content_erased OR (deleted AND name='' AND subject='' AND comment=''
        AND trip IS NULL AND poster_id IS NULL AND json_op_poster_id IS NULL AND capcode IS NULL
        AND country IS NULL AND country_name IS NULL AND board_flag IS NULL AND flag_name IS NULL
        AND wordfilter_payload IS NULL AND wordfilter_search IS NULL
        AND drawing_time_seconds IS NULL AND drawing_source_post_id IS NULL
        AND dice_result IS NULL AND fortune_text IS NULL AND fortune_color IS NULL
        AND comment_format=0 AND NOT staff_authorized_limits AND NOT image_spoiler
        AND board_flag_type='pol' AND created_at='epoch'::timestamptz));
ALTER TABLE content.threads ADD CONSTRAINT threads_erased_payload CHECK (
    NOT content_erased OR (deleted AND created_at='epoch'::timestamptz
        AND bumped_at='epoch'::timestamptz AND modified_at='epoch'::timestamptz
        AND http_modified_at='epoch'::timestamptz AND reply_count=0 AND NOT sticky
        AND NOT closed AND NOT permasage AND NOT permaage AND NOT undead AND sticky_rank=0
        AND archived_at IS NULL AND archive_expires_at IS NULL));

GRANT USAGE ON SCHEMA content,post_secrets,staff_identity TO board_content_erasure_owner;
GRANT SELECT(slug,staff_only),UPDATE(slug) ON content.boards TO board_content_erasure_owner;
GRANT SELECT(id,board,thread_id,deleted,content_erased) ON content.posts TO board_content_erasure_owner;
GRANT UPDATE(deleted) ON content.posts TO board_content_erasure_owner;
-- The posts_board_flag_check constraint invokes this finite immutable label
-- lookup during the descendant UPDATE. No other post/thread CHECK or generated
-- expression references an application function in the current schema.
GRANT EXECUTE ON FUNCTION content.board_flag_label(text,text) TO board_content_erasure_owner;
GRANT SELECT(id,board,deleted,content_erased) ON content.threads TO board_content_erasure_owner;
CREATE POLICY content_erasure_board_read ON content.boards FOR SELECT
    TO board_content_erasure_owner USING(true);
CREATE POLICY content_erasure_board_lock ON content.boards FOR UPDATE
    TO board_content_erasure_owner USING(true) WITH CHECK(true);
GRANT SELECT(post_id),DELETE ON post_secrets.deletion,post_secrets.anonymous_posts,
    post_secrets.op_replies,post_secrets.poster_contexts,post_secrets.posting_history,
    staff_identity.discussion_posts TO board_content_erasure_owner;
GRANT SELECT(thread_id),DELETE ON post_secrets.op_peers TO board_content_erasure_owner;
-- Existing guards run under their own restricted owner and need only the marker.
GRANT SELECT(content_erased) ON content.posts TO board_attachment_owner;

GRANT CREATE ON SCHEMA content,post_secrets TO board_content_erasure_owner;
SET ROLE board_content_erasure_owner;
-- The erasure owner relies on the existing RLS visibility chain. Refuse policy
-- drift that could turn a required DELETE/UPDATE into a silent zero-row result.
-- This helper reads catalogs only; readiness needs no private-schema access.
CREATE FUNCTION content.erasure_visibility_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY INVOKER SET search_path=pg_catalog,pg_temp AS $$
WITH owner_role AS (
    SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_content_erasure_owner'
), required_relations(schema_name,table_name,has_rls) AS (VALUES
    ('content','boards',true),('content','posts',true),('content','threads',true),
    ('post_secrets','deletion',true),('post_secrets','op_peers',true),
    ('post_secrets','op_replies',true),('post_secrets','anonymous_posts',false),
    ('post_secrets','poster_contexts',false),('post_secrets','posting_history',false),
    ('staff_identity','discussion_posts',false)
), relations AS (
    SELECT c.oid,c.relrowsecurity,c.relforcerowsecurity,c.relkind,c.relpersistence,r.*
    FROM required_relations r JOIN pg_catalog.pg_namespace n ON n.nspname=r.schema_name
    JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=r.table_name
)
SELECT EXISTS(SELECT 1 FROM owner_role)
AND (SELECT count(*)=10 FROM relations)
AND NOT EXISTS(SELECT 1 FROM relations r
    WHERE r.relrowsecurity<>r.has_rls OR r.relforcerowsecurity
        OR r.relkind<>'r' OR r.relpersistence<>'p')
AND NOT EXISTS(SELECT 1 FROM (VALUES
    ('content','posts','post_visibility','existsselect1fromcontent.boardsbwhereb.slug=posts.board'),
    ('content','threads','thread_visibility','existsselect1fromcontent.boardsbwhereb.slug=threads.board'),
    ('post_secrets','deletion','deletion_visibility','existsselect1fromcontent.postspwherep.id=deletion.post_id'),
    ('post_secrets','op_peers','op_peer_visibility','existsselect1fromcontent.threadstwheret.id=op_peers.thread_id'),
    ('post_secrets','op_replies','op_reply_visibility','existsselect1fromcontent.postspwherep.id=op_replies.post_id')
) required(schema_name,table_name,policy_name,expression)
WHERE NOT EXISTS(SELECT 1 FROM pg_catalog.pg_policy p JOIN relations r ON r.oid=p.polrelid
    WHERE r.schema_name=required.schema_name AND r.table_name=required.table_name
      AND p.polname=required.policy_name AND p.polcmd='*' AND p.polpermissive
      AND p.polroles=ARRAY[0]::oid[] AND p.polwithcheck IS NULL
      AND lower(regexp_replace(pg_catalog.pg_get_expr(p.polqual,p.polrelid),'[[:space:]()]','','g'))=required.expression))
AND NOT EXISTS(SELECT 1 FROM (VALUES
    ('content_erasure_board_read','r',false),('content_erasure_board_lock','w',true)
) required(policy_name,command,with_check)
WHERE NOT EXISTS(SELECT 1 FROM pg_catalog.pg_policy p JOIN relations r ON r.oid=p.polrelid
    WHERE r.schema_name='content' AND r.table_name='boards' AND p.polname=required.policy_name
      AND p.polcmd::text=required.command AND p.polpermissive
      AND p.polroles=ARRAY[(SELECT oid FROM owner_role)]::oid[]
      AND pg_catalog.pg_get_expr(p.polqual,p.polrelid)='true'
      AND CASE WHEN required.with_check THEN pg_catalog.pg_get_expr(p.polwithcheck,p.polrelid)='true'
          ELSE p.polwithcheck IS NULL END))
AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_policy p JOIN relations r ON r.oid=p.polrelid
    WHERE NOT p.polpermissive AND EXISTS(
        SELECT 1 FROM unnest(p.polroles) AS policy_role(oid)
        WHERE CASE WHEN policy_role.oid=0 THEN true
            ELSE pg_catalog.pg_has_role((SELECT oid FROM owner_role),policy_role.oid,'USAGE') END))
$$;
REVOKE ALL ON FUNCTION content.erasure_visibility_ready() FROM PUBLIC,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION content.erasure_visibility_ready() TO board_public,board_staff;

-- Check once per INSERT/UPDATE statement, including statements with no visible
-- rows. Row guards still enforce identities, board locks and irreversible data.
CREATE FUNCTION post_secrets.guard_erasure_visibility_statement() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF content.erasure_visibility_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Content erasure visibility is unavailable.' USING ERRCODE='42501';
    END IF;
    RETURN NULL;
END $$;

CREATE FUNCTION post_secrets.guard_post_erasure() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE parent_erased boolean; invoker text;
BEGIN
    IF TG_OP='INSERT' THEN
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content insertion requires Read Committed.' USING ERRCODE='22023';
        END IF;
        IF NEW.content_erased OR NEW.deleted THEN
            RAISE EXCEPTION 'Cannot insert deleted content.' USING ERRCODE='23514';
        END IF;
        -- Serialize even direct runtime inserts with thread erasure. Existing
        -- posting already holds this lock before thread/post/secret tuples.
        PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE;
        IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
        IF EXISTS(SELECT 1 FROM content.threads t WHERE t.board=NEW.board
            AND t.id=NEW.thread_id AND t.content_erased) THEN
            RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    -- Post numbers are durable identities, including media consumption links.
    IF NEW.id<>OLD.id THEN
        RAISE EXCEPTION 'Post identity cannot be reassigned.' USING ERRCODE='23514';
    END IF;
    IF NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id THEN
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content reassignment requires Read Committed.' USING ERRCODE='22023';
        END IF;
        PERFORM b.slug FROM content.boards b WHERE b.slug IN(OLD.board,NEW.board) ORDER BY b.slug FOR UPDATE NOWAIT;
        IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
        IF EXISTS(SELECT 1 FROM content.threads t WHERE t.board=NEW.board
            AND t.id=NEW.thread_id AND t.content_erased) THEN
            RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='23514';
        END IF;
    END IF;
    IF OLD.content_erased THEN
        IF NOT NEW.content_erased OR NOT NEW.deleted OR NEW.id<>OLD.id
            OR NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id
            OR to_jsonb(NEW) IS DISTINCT FROM to_jsonb(OLD) THEN
            RAISE EXCEPTION 'Erased content cannot be restored.' USING ERRCODE='23514';
        END IF;
    ELSE
        IF current_setting('transaction_isolation')<>'read committed'
            AND ((NOT OLD.deleted AND NEW.deleted) OR NEW.content_erased) THEN
            RAISE EXCEPTION 'Content erasure requires Read Committed.' USING ERRCODE='22023';
        END IF;
        -- A tuple-first operator write must fail rather than wait backwards.
        IF NEW.deleted THEN
            PERFORM b.slug FROM content.boards b WHERE b.slug=OLD.board FOR UPDATE NOWAIT;
            IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
            SELECT t.content_erased INTO parent_erased FROM content.threads t
                WHERE t.board=OLD.board AND t.id=OLD.thread_id;
        END IF;
        IF NOT ((NOT OLD.deleted AND NEW.deleted) OR (NEW.deleted AND coalesce(parent_erased,false))) THEN
            IF NEW.content_erased THEN
                RAISE EXCEPTION 'Erasure requires a whole-deletion transition.' USING ERRCODE='23514';
            END IF;
            RETURN NEW;
        END IF;
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content erasure requires Read Committed.' USING ERRCODE='22023';
        END IF;
        IF NEW.id<>OLD.id OR NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id THEN
            RAISE EXCEPTION 'Deletion cannot reassign content.' USING ERRCODE='23514';
        END IF;
        invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
        IF EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=OLD.board AND b.staff_only)
            AND invoker NOT IN ('board_staff','board_migrator') THEN
            RAISE EXCEPTION 'Content is unavailable.' USING ERRCODE='42501';
        END IF;
        DELETE FROM post_secrets.deletion WHERE post_id=OLD.id;
        DELETE FROM post_secrets.anonymous_posts WHERE post_id=OLD.id;
        DELETE FROM post_secrets.op_replies WHERE post_id=OLD.id;
        DELETE FROM post_secrets.poster_contexts WHERE post_id=OLD.id;
        DELETE FROM post_secrets.posting_history WHERE post_id=OLD.id;
        DELETE FROM staff_identity.discussion_posts WHERE post_id=OLD.id;
        IF OLD.id=OLD.thread_id THEN DELETE FROM post_secrets.op_peers WHERE thread_id=OLD.id; END IF;
    END IF;
    NEW.content_erased:=true;
    NEW.name:=''; NEW.subject:=''; NEW.comment:='';
    NEW.trip:=NULL; NEW.poster_id:=NULL; NEW.json_op_poster_id:=NULL; NEW.capcode:=NULL;
    NEW.country:=NULL; NEW.country_name:=NULL; NEW.board_flag:=NULL; NEW.flag_name:=NULL;
    NEW.wordfilter_payload:=NULL; NEW.wordfilter_search:=NULL;
    NEW.drawing_time_seconds:=NULL; NEW.drawing_source_post_id:=NULL;
    NEW.dice_result:=NULL; NEW.fortune_text:=NULL; NEW.fortune_color:=NULL;
    NEW.comment_format:=0; NEW.staff_authorized_limits:=false; NEW.image_spoiler:=false;
    NEW.board_flag_type:='pol'; NEW.created_at:='epoch'::timestamptz;
    RETURN NEW;
END $$;

CREATE FUNCTION post_secrets.guard_thread_erasure() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.content_erased OR NEW.deleted THEN
            RAISE EXCEPTION 'Cannot insert deleted content.' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    -- No application operation moves a thread identity. Operators must not
    -- move it either: author-link writers hold the captured board lock, and
    -- changing that mapping would invalidate their post-lock checks.
    IF NEW.id<>OLD.id OR NEW.board<>OLD.board THEN
        RAISE EXCEPTION 'Thread identity cannot be reassigned.' USING ERRCODE='23514';
    END IF;
    IF OLD.content_erased THEN
        IF NOT NEW.content_erased OR NOT NEW.deleted OR NEW.id<>OLD.id OR NEW.board<>OLD.board
            OR (to_jsonb(NEW)-'http_modified_at') IS DISTINCT FROM (to_jsonb(OLD)-'http_modified_at') THEN
            RAISE EXCEPTION 'Erased content cannot be restored.' USING ERRCODE='23514';
        END IF;
    ELSIF NOT OLD.deleted AND NEW.deleted THEN
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content erasure requires Read Committed.' USING ERRCODE='22023';
        END IF;
        PERFORM b.slug FROM content.boards b WHERE b.slug=OLD.board FOR UPDATE NOWAIT;
        IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
        IF NEW.id<>OLD.id OR NEW.board<>OLD.board THEN
            RAISE EXCEPTION 'Deletion cannot reassign content.' USING ERRCODE='23514';
        END IF;
    ELSE
        IF NEW.content_erased THEN
            RAISE EXCEPTION 'Erasure requires a whole-deletion transition.' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    NEW.content_erased:=true;
    NEW.created_at:='epoch'::timestamptz; NEW.bumped_at:='epoch'::timestamptz;
    NEW.modified_at:='epoch'::timestamptz; NEW.http_modified_at:='epoch'::timestamptz;
    NEW.reply_count:=0; NEW.sticky:=false; NEW.closed:=false; NEW.permasage:=false;
    NEW.permaage:=false; NEW.undead:=false; NEW.sticky_rank:=0;
    NEW.archived_at:=NULL; NEW.archive_expires_at:=NULL;
    RETURN NEW;
END $$;

CREATE FUNCTION post_secrets.erase_thread_descendants() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    -- The parent guard already enforces RC and owns the board lock. Include
    -- old soft-deleted children, but do not sweep unrelated historical threads.
    UPDATE content.posts SET deleted=true WHERE board=NEW.board AND thread_id=NEW.id AND NOT content_erased;
    -- One post-transition check also covers a zero-row descendant update.
    IF content.erasure_visibility_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Content erasure visibility is unavailable.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;

-- Protect every linked author row against recreation/reassignment after erasure.
-- Existing live registration and ownership checks remain in their callers.
CREATE FUNCTION post_secrets.guard_erased_author_link() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE target_board text; old_board text; target_id bigint; old_id bigint;
    target_thread bigint; old_thread bigint; invoker text;
BEGIN
    -- Staff associations are checked again when their deferred trigger runs,
    -- before visibility can cause its early return. Other links were already
    -- checked by this statement's preflight.
    IF TG_TABLE_SCHEMA='staff_identity' AND TG_TABLE_NAME='discussion_posts' THEN
        IF content.erasure_visibility_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Content erasure visibility is unavailable.' USING ERRCODE='42501';
        END IF;
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Author registration requires Read Committed.' USING ERRCODE='22023';
    END IF;
    target_id:=(to_jsonb(NEW)->>CASE WHEN TG_TABLE_NAME='op_peers' THEN 'thread_id' ELSE 'post_id' END)::bigint;
    IF TG_TABLE_NAME='discussion_posts' AND NOT EXISTS(SELECT 1 FROM staff_identity.discussion_posts d WHERE d.post_id=target_id) THEN
        RETURN NULL;
    END IF;
    IF TG_OP='UPDATE' THEN
        old_id:=(to_jsonb(OLD)->>CASE WHEN TG_TABLE_NAME='op_peers' THEN 'thread_id' ELSE 'post_id' END)::bigint;
    END IF;
    IF TG_TABLE_NAME='op_peers' THEN
        SELECT t.board INTO target_board FROM content.threads t WHERE t.id=target_id;
        IF TG_OP='UPDATE' THEN SELECT t.board INTO old_board FROM content.threads t WHERE t.id=old_id; END IF;
    ELSE
        SELECT p.board,p.thread_id INTO target_board,target_thread FROM content.posts p WHERE p.id=target_id;
        IF TG_OP='UPDATE' THEN
            SELECT p.board,p.thread_id INTO old_board,old_thread FROM content.posts p WHERE p.id=old_id;
        END IF;
    END IF;
    IF target_board IS NULL THEN RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='23514'; END IF;
    IF TG_OP='UPDATE' OR TG_TABLE_NAME='discussion_posts' THEN
        PERFORM b.slug FROM content.boards b WHERE b.slug IN(target_board,old_board) ORDER BY b.slug FOR UPDATE NOWAIT;
    ELSE
        PERFORM b.slug FROM content.boards b WHERE b.slug=target_board FOR UPDATE;
    END IF;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
    -- A privileged move can commit while an INSERT waits for its original
    -- board. Never continue under a stale board lock or acquire a new lock in
    -- tuple-first order. Recheck both structural mappings using the new RC
    -- statement snapshot; disappeared targets fail closed as well.
    IF TG_TABLE_NAME='op_peers' THEN
        IF NOT EXISTS(SELECT 1 FROM content.threads t WHERE t.id=target_id AND t.board=target_board)
            OR (TG_OP='UPDATE' AND NOT EXISTS(SELECT 1 FROM content.threads t
                WHERE t.id=old_id AND t.board=old_board)) THEN
            RAISE EXCEPTION 'Author target changed; retry the transaction.' USING ERRCODE='23514';
        END IF;
    ELSE
        IF NOT EXISTS(SELECT 1 FROM content.posts p WHERE p.id=target_id
                AND p.board=target_board AND p.thread_id=target_thread)
            OR (TG_OP='UPDATE' AND NOT EXISTS(SELECT 1 FROM content.posts p WHERE p.id=old_id
                AND p.board=old_board AND p.thread_id=old_thread)) THEN
            RAISE EXCEPTION 'Author target changed; retry the transaction.' USING ERRCODE='23514';
        END IF;
    END IF;
    invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF EXISTS(SELECT 1 FROM content.boards b WHERE b.slug IN(target_board,old_board) AND b.staff_only)
        AND invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='23514';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
        WHERE p.id=target_id AND (p.content_erased OR t.content_erased))
        OR (TG_OP='UPDATE' AND EXISTS(SELECT 1 FROM content.posts p WHERE p.id=old_id AND p.content_erased))
        OR (TG_TABLE_NAME='op_peers' AND EXISTS(SELECT 1 FROM content.threads t
            WHERE t.id IN(target_id,old_id) AND t.content_erased)) THEN
        RAISE EXCEPTION 'Erased author authority cannot be restored.' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION post_secrets.guard_post_erasure(),post_secrets.guard_thread_erasure(),
    post_secrets.erase_thread_descendants(),post_secrets.guard_erased_author_link(),
    post_secrets.guard_erasure_visibility_statement()
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION post_secrets.guard_post_erasure(),post_secrets.guard_thread_erasure(),
    post_secrets.erase_thread_descendants(),post_secrets.guard_erased_author_link(),
    post_secrets.guard_erasure_visibility_statement() TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_content_erasure_owner;
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON content.posts
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON content.threads
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON post_secrets.deletion
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON post_secrets.anonymous_posts
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON post_secrets.op_peers
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON post_secrets.op_replies
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON post_secrets.poster_contexts
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON post_secrets.posting_history
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER guard_erasure_visibility BEFORE INSERT OR UPDATE ON staff_identity.discussion_posts
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.guard_erasure_visibility_statement();
CREATE TRIGGER z_guard_post_erasure BEFORE INSERT OR UPDATE ON content.posts
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_post_erasure();
-- Runs after advance_http_clock: erased metadata has no public representation.
CREATE TRIGGER z_guard_thread_erasure BEFORE INSERT OR UPDATE ON content.threads
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_thread_erasure();
CREATE TRIGGER erase_thread_descendants AFTER UPDATE OF deleted ON content.threads
    FOR EACH ROW WHEN (NOT OLD.content_erased AND NEW.content_erased)
    EXECUTE FUNCTION post_secrets.erase_thread_descendants();
CREATE TRIGGER z_guard_erased_author_link BEFORE INSERT OR UPDATE ON post_secrets.deletion
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_erased_author_link();
CREATE TRIGGER z_guard_erased_author_link BEFORE INSERT OR UPDATE ON post_secrets.anonymous_posts
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_erased_author_link();
CREATE TRIGGER z_guard_erased_author_link BEFORE INSERT OR UPDATE ON post_secrets.op_peers
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_erased_author_link();
CREATE TRIGGER z_guard_erased_author_link BEFORE INSERT OR UPDATE ON post_secrets.op_replies
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_erased_author_link();
CREATE TRIGGER z_guard_erased_author_link BEFORE INSERT OR UPDATE ON post_secrets.poster_contexts
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_erased_author_link();
CREATE TRIGGER z_guard_erased_author_link BEFORE INSERT OR UPDATE ON post_secrets.posting_history
    FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_erased_author_link();
-- Staff registration precedes the post row under its deferred FK. Check the
-- final surviving association, not that temporary pre-insert state.
CREATE CONSTRAINT TRIGGER z_guard_erased_author_link AFTER INSERT OR UPDATE ON staff_identity.discussion_posts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_erased_author_link();
SET ROLE board_content_erasure_owner;
REVOKE EXECUTE ON FUNCTION post_secrets.guard_post_erasure(),post_secrets.guard_thread_erasure(),
    post_secrets.erase_thread_descendants(),post_secrets.guard_erased_author_link(),
    post_secrets.guard_erasure_visibility_statement() FROM board_migrator;
RESET ROLE;

-- Keep live empty-comment validation, including the deferred final-row check.
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.require_attachment_for_empty_post() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM content.posts p WHERE p.id=NEW.id AND p.comment='' AND NOT p.content_erased
        AND NOT (p.id=p.thread_id AND p.subject<>''))
        AND NOT EXISTS(SELECT 1 FROM content.post_media m WHERE m.post_id=NEW.id) THEN
        RAISE EXCEPTION 'An empty comment requires an authorized attachment.'
            USING ERRCODE='23514',CONSTRAINT='posts_empty_comment_attachment';
    END IF;
    RETURN NULL;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
