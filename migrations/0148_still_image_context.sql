-- Immutable posting context for newly reserved source-image uploads.
-- Existing reservations keep their original processing path.
CREATE TABLE media.image_context (
    job_id text PRIMARY KEY REFERENCES media.jobs(id) ON DELETE CASCADE,
    board text NOT NULL CHECK (board IN ('3', 'a', 'aco', 'adv', 'an', 'asp', 'b', 'bant', 'biz', 'c', 'cgl', 'ck', 'cm', 'co', 'd', 'diy', 'e', 'f', 'fa', 'fit', 'g', 'gd', 'gif', 'h', 'hc', 'his', 'hm', 'hr', 'i', 'ic', 'int', 'j', 'jp', 'k', 'lgbt', 'lit', 'm', 'mlp', 'mu', 'n', 'news', 'o', 'out', 'p', 'po', 'pol', 'pw', 'qa', 'qb', 'qst', 'r', 'r9k', 's', 's4s', 'sci', 'soc', 'sp', 't', 'test', 'tg', 'toy', 'trash', 'trv', 'tv', 'u', 'v', 'vg', 'vip', 'vm', 'vmg', 'vp', 'vr', 'vrpg', 'vst', 'vt', 'w', 'wg', 'wsg', 'wsr', 'x', 'xs', 'y')),
    thread_id bigint NOT NULL CHECK (thread_id >= 0),
    capcode boolean NOT NULL
);
REVOKE ALL ON media.image_context FROM PUBLIC;
GRANT SELECT(job_id,board,thread_id,capcode) ON media.image_context
    TO board_media,board_attachment_owner;
GRANT INSERT(job_id,board,thread_id,capcode) ON media.image_context
    TO board_media_intake_owner;

CREATE FUNCTION media.guard_image_context() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        RAISE EXCEPTION 'Image posting context is immutable.' USING ERRCODE='42501';
    END IF;
    PERFORM 1 FROM media.jobs WHERE id=NEW.job_id AND input_kind='image-v1'
        AND state='receiving' AND expires_at>clock_timestamp() FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Image context requires a new intake reservation.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_image_context() FROM PUBLIC;
CREATE TRIGGER media_image_context_immutable BEFORE INSERT OR UPDATE ON media.image_context
    FOR EACH ROW EXECUTE FUNCTION media.guard_image_context();

CREATE FUNCTION media_intake.reserve_image(p_filename text,p_board text,p_thread bigint,p_capcode boolean)
RETURNS TABLE(id text,capability text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE reservation record;
BEGIN
    IF p_board IS NULL OR p_board NOT IN ('3', 'a', 'aco', 'adv', 'an', 'asp', 'b', 'bant', 'biz', 'c', 'cgl', 'ck', 'cm', 'co', 'd', 'diy', 'e', 'f', 'fa', 'fit', 'g', 'gd', 'gif', 'h', 'hc', 'his', 'hm', 'hr', 'i', 'ic', 'int', 'j', 'jp', 'k', 'lgbt', 'lit', 'm', 'mlp', 'mu', 'n', 'news', 'o', 'out', 'p', 'po', 'pol', 'pw', 'qa', 'qb', 'qst', 'r', 'r9k', 's', 's4s', 'sci', 'soc', 'sp', 't', 'test', 'tg', 'toy', 'trash', 'trv', 'tv', 'u', 'v', 'vg', 'vip', 'vm', 'vmg', 'vp', 'vr', 'vrpg', 'vst', 'vt', 'w', 'wg', 'wsg', 'wsr', 'x', 'xs', 'y')
        OR p_thread IS NULL OR p_thread<0 OR p_capcode IS NULL THEN
        RAISE EXCEPTION 'Invalid image posting context.' USING ERRCODE='22023';
    END IF;
    SELECT * INTO STRICT reservation FROM media_intake.reserve(p_filename);
    INSERT INTO media.image_context(job_id,board,thread_id,capcode)
        VALUES(reservation.id,p_board,p_thread,p_capcode);
    RETURN QUERY SELECT reservation.id,reservation.capability;
END $$;
REVOKE ALL ON FUNCTION media_intake.reserve_image(text,text,bigint,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media_intake.reserve_image(text,text,bigint,boolean) TO board_media_intake;
GRANT CREATE ON SCHEMA media_intake TO board_media_intake_owner;
ALTER FUNCTION media_intake.reserve_image(text,text,bigint,boolean) OWNER TO board_media_intake_owner;
REVOKE CREATE ON SCHEMA media_intake FROM board_media_intake_owner;
