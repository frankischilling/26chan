-- Password rotation and revocation share the lock used by public mutations.
-- The trigger runs with the caller's privileges and grants no new authority.
CREATE FUNCTION post_secrets.lock_deletion_authority() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    next_post bigint := OLD.post_id;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        next_post := NEW.post_id;
    END IF;
    -- A reassignment can affect two boards; acquire those locks in slug order.
    PERFORM b.slug FROM content.boards b
        WHERE EXISTS (SELECT 1 FROM content.posts p
            WHERE p.board = b.slug AND p.id IN (OLD.post_id, next_post))
        ORDER BY b.slug FOR UPDATE OF b;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION post_secrets.lock_deletion_authority() FROM PUBLIC;

CREATE TRIGGER deletion_authority_mutation
    BEFORE UPDATE OR DELETE ON post_secrets.deletion
    FOR EACH ROW EXECUTE FUNCTION post_secrets.lock_deletion_authority();
