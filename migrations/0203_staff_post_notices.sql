DO $$ BEGIN IF encode(sha256(convert_to(pg_get_viewdef('site_private.frontpage_eligible'::regclass,true),'UTF8')),'hex') IS DISTINCT FROM 'd3a0608ad915ca0bce103a29b02dfdc2e7b43c0e2261abda1bb485da8b3eed8c' THEN RAISE EXCEPTION 'Homepage projection drifted before post notices'; END IF; END $$;

-- Ordered staff notices remain separate from immutable comment presentation.
-- Existing posts start with no notice; no historical content is backfilled.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer authority drifted before post notices'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report snapshots drifted before post notices'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction authority drifted before post notices'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report profile drifted before post notices'; END IF; END $$;
ALTER TABLE content.posts ADD COLUMN staff_notices text[] NOT NULL DEFAULT '{}';
ALTER TABLE content.posts ADD CONSTRAINT posts_staff_notices CHECK (
 cardinality(staff_notices)<=16 AND (cardinality(staff_notices)=0 OR
 (array_ndims(staff_notices)=1 AND array_lower(staff_notices,1)=1))
 AND array_position(staff_notices,NULL) IS NULL AND array_position(staff_notices,'') IS NULL
 AND coalesce(octet_length(staff_notices[1]),0)<=4096
 AND coalesce(octet_length(staff_notices[2]),0)<=4096
 AND coalesce(octet_length(staff_notices[3]),0)<=4096
 AND coalesce(octet_length(staff_notices[4]),0)<=4096
 AND coalesce(octet_length(staff_notices[5]),0)<=4096
 AND coalesce(octet_length(staff_notices[6]),0)<=4096
 AND coalesce(octet_length(staff_notices[7]),0)<=4096
 AND coalesce(octet_length(staff_notices[8]),0)<=4096
 AND coalesce(octet_length(staff_notices[9]),0)<=4096
 AND coalesce(octet_length(staff_notices[10]),0)<=4096
 AND coalesce(octet_length(staff_notices[11]),0)<=4096
 AND coalesce(octet_length(staff_notices[12]),0)<=4096
 AND coalesce(octet_length(staff_notices[13]),0)<=4096
 AND coalesce(octet_length(staff_notices[14]),0)<=4096
 AND coalesce(octet_length(staff_notices[15]),0)<=4096
 AND coalesce(octet_length(staff_notices[16]),0)<=4096
);


ALTER TABLE content.posts DROP CONSTRAINT posts_erased_payload;
ALTER TABLE content.posts ADD CONSTRAINT posts_erased_payload CHECK (
    NOT content_erased OR (deleted AND NOT file_deleted AND name='' AND subject='' AND comment=''
        AND staff_notices='{}'::text[] AND trip IS NULL AND poster_id IS NULL AND json_op_poster_id IS NULL AND capcode IS NULL
        AND country IS NULL AND country_name IS NULL AND board_flag IS NULL AND flag_name IS NULL
        AND wordfilter_payload IS NULL AND wordfilter_search IS NULL
        AND drawing_time_seconds IS NULL AND drawing_source_post_id IS NULL
        AND dice_result IS NULL AND fortune_text IS NULL AND fortune_color IS NULL
        AND comment_format=0 AND NOT staff_authorized_limits AND NOT image_spoiler
        AND board_flag_type='pol' AND created_at='epoch'::timestamptz));

GRANT CREATE ON SCHEMA post_secrets TO board_content_erasure_owner;
SET LOCAL ROLE board_content_erasure_owner;

CREATE OR REPLACE FUNCTION post_secrets.guard_post_erasure() RETURNS trigger
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
        invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
        -- Only the existing migration operator may explicitly erase an old
        -- tombstone. Ordinary deletion authority and live transitions do not change.
        IF NOT ((NOT OLD.deleted AND NEW.deleted) OR (NEW.deleted AND coalesce(parent_erased,false))
            OR (OLD.deleted AND NEW.deleted AND NEW.content_erased AND invoker='board_migrator')) THEN
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
    NEW.name:=''; NEW.subject:=''; NEW.comment:=''; NEW.staff_notices:='{}'::text[];
    NEW.trip:=NULL; NEW.poster_id:=NULL; NEW.json_op_poster_id:=NULL; NEW.capcode:=NULL;
    NEW.country:=NULL; NEW.country_name:=NULL; NEW.board_flag:=NULL; NEW.flag_name:=NULL;
    NEW.wordfilter_payload:=NULL; NEW.wordfilter_search:=NULL;
    NEW.saved_presentation:=NULL; NEW.saved_presentation_search:=NULL;
    NEW.drawing_time_seconds:=NULL; NEW.drawing_source_post_id:=NULL;
    NEW.dice_result:=NULL; NEW.fortune_text:=NULL; NEW.fortune_color:=NULL;
    NEW.file_deleted:=false;
    NEW.comment_format:=0; NEW.staff_authorized_limits:=false; NEW.image_spoiler:=false;
    NEW.board_flag_type:='pol'; NEW.created_at:='epoch'::timestamptz;
    RETURN NEW;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_content_erasure_owner;

CREATE OR REPLACE VIEW content.report_snapshot_source WITH (security_barrier=true) AS
SELECT p.board,p.id AS post_id,
 jsonb_build_object(
  'board',p.board,'post_id',p.id,'thread_id',p.thread_id,
  'created_at',p.created_at,'name',p.name,'trip',p.trip,'poster_id',p.poster_id,
  'capcode',p.capcode,'country',p.country,'country_name',p.country_name,
  'board_flag',p.board_flag,'board_flag_type',p.board_flag_type,'flag_name',p.flag_name,
  'subject',p.subject,'comment',p.comment,'staff_notices',to_jsonb(p.staff_notices),'comment_format',p.comment_format,'file_deleted',p.file_deleted,
  'staff_authorized_limits',p.staff_authorized_limits,
  'wordfilter_payload',encode(p.wordfilter_payload,'hex'),
  'saved_presentation',CASE WHEN to_jsonb(p)->>'saved_presentation' IS NOT NULL
      THEN substring(to_jsonb(p)->>'saved_presentation' FROM 3) END,
  'drawing_time_seconds',p.drawing_time_seconds,
  'drawing_source_post_id',p.drawing_source_post_id,
  'archived',t.archived_at IS NOT NULL,
  'relative_subject',CASE WHEN p.id<>p.thread_id THEN (
      SELECT op.subject FROM content.posts op WHERE op.id=p.thread_id AND op.board=p.board) END,
  'attachment',CASE WHEN a.post_id IS NOT NULL THEN jsonb_build_object(
      'post_id',a.post_id,'filename',a.filename,'bytes',a.bytes,'width',a.width,'height',a.height,
      'spoiler',a.spoiler,'tim',a.tim,'thumbnail_width',a.thumbnail_width,
      'thumbnail_height',a.thumbnail_height,'available',a.available,
      'md5',a.md5,'output_format',a.output_format) END
 ) AS payload
FROM content.posts p
JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
LEFT JOIN content.staff_post_media a ON a.post_id=p.id
WHERE NOT p.deleted AND NOT t.deleted
  AND (t.archived_at IS NULL OR t.archive_expires_at>clock_timestamp());

GRANT SELECT(staff_notices),INSERT(staff_notices) ON content.posts TO board_thread_transfer_owner;
GRANT CREATE ON SCHEMA content,post_secrets,staff_identity TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;

CREATE OR REPLACE FUNCTION post_secrets.transfer_post_hash(p_id bigint) RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 SELECT sha256(convert_to(jsonb_build_array(p.id,p.board,p.thread_id,p.name,p.trip,p.subject,p.comment,p.staff_notices,
    p.created_at,p.deleted,p.content_erased,p.capcode,p.poster_id,p.json_op_poster_id,p.country,p.country_name,
    p.board_flag,p.board_flag_type,p.flag_name,p.image_spoiler,p.file_deleted,p.comment_format,
    p.staff_authorized_limits,encode(p.wordfilter_payload,'hex'),p.wordfilter_search,p.drawing_time_seconds,
    p.drawing_source_post_id,p.dice_result,p.fortune_text,p.fortune_color,
    encode(p.saved_presentation,'hex'),p.saved_presentation_search,
    (SELECT jsonb_build_array(h.automatic_browser,encode(h.automatic_prefix,'hex'))
       FROM post_secrets.posting_history h WHERE h.post_id=p.id AND h.board=p.board))::text,'UTF8'))
 FROM content.posts p WHERE p.id=p_id
$$;

CREATE OR REPLACE FUNCTION post_secrets.transfer_lock_source(p_source text,p_thread bigint,p_destination text)
RETURNS integer LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_count integer; v_bytes bigint; v_all_bytes bigint;
BEGIN
    IF p_source IS NULL OR p_destination IS NULL OR p_source !~ '^[a-z0-9]{1,10}$'
        OR p_destination !~ '^[a-z0-9]{1,10}$' OR p_source=p_destination OR p_source='b'
        OR p_destination IN('f','j') OR p_thread IS NULL OR p_thread<=0 THEN
        RAISE EXCEPTION 'Invalid transfer target.' USING ERRCODE='P131I';
    END IF;
    PERFORM b.slug FROM content.boards b WHERE b.slug IN(p_source,p_destination)
        ORDER BY b.slug FOR UPDATE NOWAIT;
    IF (SELECT count(*) FROM content.boards b WHERE b.slug IN(p_source,p_destination)
        AND NOT b.staff_only AND NOT b.upload_board)<>2 THEN
        RAISE EXCEPTION 'Transfer board is unavailable.' USING ERRCODE='P131N';
    END IF;
    PERFORM t.id FROM content.threads t WHERE t.board=p_source AND t.id=p_thread
        AND NOT t.deleted AND NOT t.content_erased AND NOT t.closed AND t.archived_at IS NULL FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer source is unavailable.' USING ERRCODE='P131N'; END IF;
    -- Retained sticky tombstones can outgrow the live window. Bound all work,
    -- including structural-media inspection, rather than sweeping that history.
    IF (SELECT count(*) FROM (SELECT 1 FROM content.posts p WHERE p.board=p_source
        AND p.thread_id=p_thread LIMIT 1002) bounded)>1001 THEN
        RAISE EXCEPTION 'Transfer source exceeds its bounded window.' USING ERRCODE='P131L';
    END IF;
    PERFORM p.id FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread ORDER BY p.id FOR UPDATE NOWAIT;
    IF NOT EXISTS(SELECT 1 FROM content.posts p WHERE p.board=p_source AND p.id=p_thread
        AND p.thread_id=p_thread AND NOT p.deleted AND NOT p.content_erased) THEN
        RAISE EXCEPTION 'Transfer source is unavailable.' USING ERRCODE='P131N';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p JOIN content.post_media m ON m.post_id=p.id
        WHERE p.board=p_source AND p.thread_id=p_thread) THEN
        RAISE EXCEPTION 'Only media-free threads can be transferred.' USING ERRCODE='P131F';
    END IF;
    SELECT count(*),coalesce(sum(octet_length(p.comment)+CASE WHEN cardinality(p.staff_notices)=0 THEN 0 ELSE octet_length(p.staff_notices::text) END::bigint+coalesce(octet_length(p.wordfilter_payload),0)
        +coalesce(octet_length(p.wordfilter_search),0)+coalesce(octet_length(p.saved_presentation),0)
        +coalesce(octet_length(p.saved_presentation_search),0)),0),
        coalesce(sum(octet_length(p.comment)+CASE WHEN cardinality(p.staff_notices)=0 THEN 0 ELSE octet_length(p.staff_notices::text) END::bigint+coalesce(octet_length(p.wordfilter_payload),0)
        +coalesce(octet_length(p.wordfilter_search),0)+coalesce(octet_length(p.saved_presentation),0)
        +coalesce(octet_length(p.saved_presentation_search),0)+octet_length(p.name)+octet_length(p.subject)+2048),0)
        INTO v_count,v_bytes,v_all_bytes
    FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted;
    IF v_count NOT BETWEEN 1 AND 1001 OR v_bytes>64064000 OR v_all_bytes>67108864 THEN
        RAISE EXCEPTION 'Transfer exceeds its capacity.' USING ERRCODE='P131L';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted
        AND (p.content_erased OR NOT isfinite(p.created_at)
            OR NOT (p.comment_format BETWEEN 104 AND 111 OR p.comment_format BETWEEN 120 AND 127
                OR p.comment_format BETWEEN 232 AND 239 OR p.comment_format BETWEEN 248 AND 255)
            OR p.drawing_time_seconds IS NOT NULL OR p.drawing_source_post_id IS NOT NULL
            OR p.dice_result IS NOT NULL OR p.fortune_text IS NOT NULL OR p.fortune_color IS NOT NULL
            OR (p.saved_presentation IS NOT NULL AND p.wordfilter_payload IS NOT NULL))) THEN
        RAISE EXCEPTION 'Source presentation cannot be transferred.' USING ERRCODE='P131F';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p JOIN content.boards b ON b.slug=p.board
        WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted
        AND b.country_flags AND p.board_flag IS NULL AND (p.country IS NULL OR p.country_name IS NULL)) THEN
        RAISE EXCEPTION 'Source country evidence is unavailable.' USING ERRCODE='P131F';
    END IF;
    RETURN v_count;
END $$;

CREATE OR REPLACE FUNCTION staff_identity.seal_text_thread_transfer(p_ticket bytea,p_session bytea,p_csrf bytea,
    p_fingerprint bytea,p_version smallint,p_old bigint[],p_new bigint[],p_comment text[],p_fp bytea[],p_search text[])
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE i post_secrets.thread_transfer_intents%ROWTYPE; v_account bigint; n integer; total bigint:=0; m record;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_auth' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM post_secrets.transfer_gate();
    SELECT * INTO i FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    IF p_session IS DISTINCT FROM i.session_hash OR p_csrf IS DISTINCT FROM i.csrf_hash THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    v_account:=post_secrets.transfer_authority(p_session,p_csrf,i.idle_seconds,i.source_board,i.destination_board,true,true,i.credential_id);
    PERFORM ticket_hash FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket FOR UPDATE NOWAIT;
    IF v_account<>i.account_id OR i.expires_at<=clock_timestamp() THEN
        RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    IF i.sealed OR p_fingerprint IS DISTINCT FROM i.source_fingerprint OR p_version IS DISTINCT FROM i.formatter_version
        OR cardinality(p_old) IS DISTINCT FROM i.post_count OR cardinality(p_new) IS DISTINCT FROM i.post_count
        OR cardinality(p_comment) IS DISTINCT FROM i.post_count OR cardinality(p_fp) IS DISTINCT FROM i.post_count
        OR cardinality(p_search) IS DISTINCT FROM i.post_count
        OR array_ndims(p_old)<>1 OR array_ndims(p_new)<>1 OR array_ndims(p_comment)<>1
        OR array_ndims(p_fp)<>1 OR array_ndims(p_search)<>1
        OR array_lower(p_old,1)<>1 OR array_lower(p_new,1)<>1 OR array_lower(p_comment,1)<>1
        OR array_lower(p_fp,1)<>1 OR array_lower(p_search,1)<>1 THEN
        RAISE EXCEPTION 'Invalid sealed transfer.' USING ERRCODE='P131I'; END IF;
    FOR m IN SELECT * FROM post_secrets.thread_transfer_map WHERE ticket_hash=p_ticket ORDER BY ordinal LOOP
        n:=m.ordinal;
        IF p_old[n] IS DISTINCT FROM m.source_post_id OR p_new[n] IS DISTINCT FROM m.destination_post_id
            OR p_comment[n] IS NULL OR p_fp[n] IS NULL OR p_search[n] IS NULL THEN
            RAISE EXCEPTION 'Invalid sealed transfer.' USING ERRCODE='P131I'; END IF;
        IF octet_length(p_fp[n]) NOT BETWEEN 9 AND (CASE WHEN m.authorized THEN 524288 ELSE 131072 END)
            OR substring(p_fp[n] FROM 1 FOR 4)<>decode(CASE WHEN m.authorized THEN '46503032' ELSE '46503031' END,'hex')
            OR octet_length(p_comment[n])>(CASE WHEN m.authorized THEN 2097152 ELSE 524288 END)
            OR octet_length(p_search[n])>(CASE WHEN m.authorized THEN 524288 ELSE 131072 END) THEN
            RAISE EXCEPTION 'Sealed presentation exceeds its bounds.' USING ERRCODE='P131F'; END IF;
        IF get_byte(p_fp[n],4)<>m.wordfiltered::integer THEN
            RAISE EXCEPTION 'Invalid sealed provenance.' USING ERRCODE='P131F'; END IF;
        total:=total+octet_length(p_comment[n])+octet_length(p_fp[n])+octet_length(p_search[n]) + (SELECT CASE WHEN cardinality(p.staff_notices)=0 THEN 0 ELSE octet_length(p.staff_notices::text) END FROM content.posts p WHERE p.id=m.source_post_id);
        IF total>64064000 THEN RAISE EXCEPTION 'Transfer exceeds its capacity.' USING ERRCODE='P131L'; END IF;
        INSERT INTO post_secrets.thread_transfer_sealed_rows VALUES(p_ticket,n,p_comment[n],p_fp[n],p_search[n]);
    END LOOP;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    PERFORM post_secrets.transfer_authority(p_session,p_csrf,i.idle_seconds,i.source_board,i.destination_board,true,false,i.credential_id);
    UPDATE post_secrets.thread_transfer_intents SET sealed=true WHERE ticket_hash=p_ticket;
END $$;

CREATE OR REPLACE FUNCTION content.consume_text_thread_transfer(p_ticket bytea)
RETURNS TABLE(request_id text,source_board text,source_thread bigint,destination_board text,
    destination_thread bigint,post_count bigint,audit_id bigint,source_post_id bigint,destination_post_id bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE i post_secrets.thread_transfer_intents%ROWTYPE; v_count integer; v_audit bigint; v_now timestamptz;
    v_inserted integer; v_anonymous_at bigint;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_staff' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM post_secrets.transfer_gate();
    SELECT * INTO i FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    IF post_secrets.transfer_authority(i.session_hash,i.csrf_hash,i.idle_seconds,
        i.source_board,i.destination_board,true,true,i.credential_id)<>i.account_id THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM ticket_hash FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket FOR UPDATE NOWAIT;
    IF NOT i.sealed THEN RAISE EXCEPTION 'Transfer is not sealed.' USING ERRCODE='P131I'; END IF;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    v_count:=post_secrets.transfer_lock_source(i.source_board,i.source_thread,i.destination_board);
    IF v_count<>i.post_count OR post_secrets.transfer_source_hash(i.source_board,i.source_thread,i.destination_board)
        IS DISTINCT FROM i.source_fingerprint OR EXISTS(
            SELECT 1 FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket
            AND post_secrets.transfer_post_hash(m.source_post_id) IS DISTINCT FROM m.source_hash) THEN
        RAISE EXCEPTION 'Transfer source changed.' USING ERRCODE='P131C'; END IF;
    IF (SELECT count(*) FROM post_secrets.thread_transfer_map WHERE ticket_hash=p_ticket)<>v_count
        OR (SELECT count(*) FROM post_secrets.thread_transfer_sealed_rows WHERE ticket_hash=p_ticket)<>v_count
        OR EXISTS(SELECT 1 FROM post_secrets.thread_transfer_map m JOIN content.posts p ON p.id=m.destination_post_id WHERE m.ticket_hash=p_ticket)
        OR EXISTS(SELECT 1 FROM content.threads t WHERE t.id=i.destination_thread) THEN
        RAISE EXCEPTION 'Transfer destination changed.' USING ERRCODE='P131C'; END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM content.threads t WHERE t.board=i.destination_board AND NOT t.deleted
        AND t.archived_at IS NULL AND NOT t.sticky AND NOT t.undead LIMIT 1001) occupied)>=
        (SELECT b.thread_limit FROM content.boards b WHERE b.slug=i.destination_board)
        OR (SELECT count(*) FROM (SELECT 1 FROM post_secrets.posting_history h WHERE h.board=i.destination_board LIMIT 100001) occupied)
          +(SELECT count(*) FROM post_secrets.posting_history h JOIN post_secrets.thread_transfer_map m
            ON m.source_post_id=h.post_id WHERE m.ticket_hash=p_ticket)>100000 THEN
        RAISE EXCEPTION 'Transfer destination capacity is exhausted.' USING ERRCODE='P131L'; END IF;
    -- Session before membership; NOWAIT avoids reverse-order expiry cleanup.
    PERFORM s.token_hash FROM post_secrets.anonymous_sessions s WHERE EXISTS(
        SELECT 1 FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.source_post_id=a.post_id
        WHERE m.ticket_hash=p_ticket AND a.token_hash=s.token_hash) ORDER BY s.token_hash FOR UPDATE NOWAIT;
    PERFORM d.post_id FROM post_secrets.deletion d JOIN post_secrets.thread_transfer_map m ON m.source_post_id=d.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY d.post_id FOR SHARE OF d NOWAIT;
    PERFORM o.thread_id FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread FOR SHARE NOWAIT;
    PERFORM r.post_id FROM post_secrets.op_replies r JOIN post_secrets.thread_transfer_map m ON m.source_post_id=r.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY r.post_id FOR SHARE OF r NOWAIT;
    PERFORM h.post_id FROM post_secrets.posting_history h JOIN post_secrets.thread_transfer_map m ON m.source_post_id=h.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY h.post_id FOR SHARE OF h NOWAIT;
    PERFORM a.post_id FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.source_post_id=a.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY a.post_id FOR SHARE OF a NOWAIT;
    PERFORM o.post_id FROM verification_private.post_ownership o JOIN post_secrets.thread_transfer_map m ON m.source_post_id=o.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY o.post_id FOR SHARE OF o NOWAIT;
    INSERT INTO post_secrets.thread_transfer_active
        SELECT pg_current_xact_id(),m.destination_post_id,i.destination_board,i.destination_thread,p_ticket
        FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket;
    v_now:=clock_timestamp();
    INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,http_modified_at,reply_count,
        sticky,sticky_rank,closed,permasage,permaage,undead,archived_at,archive_expires_at,deleted,content_erased)
    SELECT i.destination_thread,i.destination_board,p.created_at,v_now,p.created_at,v_now,v_count-1,
        false,0,false,false,false,false,NULL,NULL,false,false
    FROM content.posts p WHERE p.id=i.source_thread AND p.board=i.source_board;
    INSERT INTO content.posts(id,board,thread_id,name,trip,subject,comment,created_at,capcode,poster_id,json_op_poster_id,
        country,country_name,board_flag,board_flag_type,flag_name,image_spoiler,file_deleted,comment_format,
        staff_authorized_limits,saved_presentation,saved_presentation_search,staff_notices)
    SELECT m.destination_post_id,i.destination_board,i.destination_thread,p.name,p.trip,p.subject,s.comment,p.created_at,
        p.capcode,NULL,NULL,CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country ELSE 'XX' END,
        CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country_name ELSE 'Unknown' END,
        NULL,'pol',NULL,false,p.file_deleted,m.comment_format,m.authorized,s.saved_presentation,s.saved_presentation_search,p.staff_notices
    FROM post_secrets.thread_transfer_map m JOIN post_secrets.thread_transfer_sealed_rows s
        ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal JOIN content.posts p ON p.id=m.source_post_id
    JOIN content.boards b ON b.slug=i.source_board WHERE m.ticket_hash=p_ticket ORDER BY m.ordinal;
    GET DIAGNOSTICS v_inserted=ROW_COUNT;
    IF v_inserted<>v_count OR EXISTS(
        SELECT 1 FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.thread_transfer_sealed_rows s ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal
        JOIN content.posts p ON p.id=m.source_post_id JOIN content.boards b ON b.slug=i.source_board
        LEFT JOIN content.posts d ON d.id=m.destination_post_id WHERE m.ticket_hash=p_ticket AND (
            d.id IS NULL OR ROW(d.board,d.thread_id,d.name,d.trip,d.subject,d.comment,d.created_at,d.capcode,
                d.poster_id,d.json_op_poster_id,d.country,d.country_name,d.board_flag,d.board_flag_type,d.flag_name,
                d.image_spoiler,d.file_deleted,d.comment_format,d.staff_authorized_limits,d.saved_presentation,
                d.saved_presentation_search,d.staff_notices,d.wordfilter_payload,d.wordfilter_search,d.drawing_time_seconds,
                d.drawing_source_post_id,d.dice_result,d.fortune_text,d.fortune_color,d.deleted,d.content_erased)
              IS DISTINCT FROM ROW(i.destination_board,i.destination_thread,p.name,p.trip,p.subject,s.comment,
                p.created_at,p.capcode,NULL::text,NULL::text,
                CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country ELSE 'XX' END,
                CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country_name ELSE 'Unknown' END,
                NULL::text,'pol'::text,NULL::text,false,p.file_deleted,m.comment_format,m.authorized,
                s.saved_presentation,s.saved_presentation_search,p.staff_notices,NULL::bytea,NULL::text,NULL::integer,
                NULL::bigint,NULL::text,NULL::text,NULL::text,false,false))) THEN
        RAISE EXCEPTION 'Transfer payload verification failed.' USING ERRCODE='P131R';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM content.threads t JOIN content.posts p ON p.id=i.source_thread
        WHERE t.id=i.destination_thread AND ROW(t.board,t.created_at,t.bumped_at,t.modified_at,t.http_modified_at,
            t.reply_count,t.sticky,t.sticky_rank,t.closed,t.permasage,t.permaage,t.undead,t.archived_at,
            t.archive_expires_at,t.deleted,t.content_erased)
          IS NOT DISTINCT FROM ROW(i.destination_board,p.created_at,v_now,p.created_at,v_now,v_count-1,
            false,0::smallint,false,false,false,false,NULL::timestamptz,NULL::timestamptz,false,false)) THEN
        RAISE EXCEPTION 'Transfer thread verification failed.' USING ERRCODE='P131R';
    END IF;
    INSERT INTO post_secrets.deletion(post_id,password_hash)
        SELECT m.destination_post_id,d.password_hash FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket;
    INSERT INTO post_secrets.op_peers(thread_id,peer)
        SELECT i.destination_thread,o.peer FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread;
    INSERT INTO post_secrets.op_replies(post_id,thread_id)
        SELECT m.destination_post_id,i.destination_thread FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.op_replies r ON r.post_id=m.source_post_id AND r.thread_id=i.source_thread
        WHERE m.ticket_hash=p_ticket;
    INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at,automatic_identity,registration_xid,automatic_browser,automatic_prefix)
        SELECT m.destination_post_id,i.destination_board,i.destination_thread,h.actor_hash,h.request_at,h.automatic_identity,
            CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid END,h.automatic_browser,h.automatic_prefix
        FROM post_secrets.thread_transfer_map m JOIN post_secrets.posting_history h ON h.post_id=m.source_post_id
            AND h.board=i.source_board AND h.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket;
    v_anonymous_at:=extract(epoch FROM clock_timestamp())::bigint;
    INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
        SELECT m.destination_post_id,a.token_hash,a.password_proof FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.anonymous_posts a ON a.post_id=m.source_post_id
        JOIN post_secrets.anonymous_sessions s ON s.token_hash=a.token_hash AND s.expires_at>v_anonymous_at
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
        WHERE m.ticket_hash=p_ticket;
    INSERT INTO verification_private.post_ownership(post_id,identity_id,password_proof)
        SELECT m.destination_post_id,o.identity_id,o.password_proof FROM post_secrets.thread_transfer_map m
        JOIN verification_private.post_ownership o ON o.post_id=m.source_post_id
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND o.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
        WHERE m.ticket_hash=p_ticket;
    -- Compare complete private row sets, not only counts. These sets and all
    -- hashes/peers/UUIDs remain inside SQL; errors contain no evidence values.
    IF EXISTS(
        WITH expected(kind,id,payload) AS (
            SELECT 'deletion',m.destination_post_id,jsonb_build_array(d.password_hash)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.deletion d ON d.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'peer',i.destination_thread,jsonb_build_array(o.peer)
            FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread
            UNION ALL SELECT 'reply',m.destination_post_id,jsonb_build_array(i.destination_thread)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.op_replies r ON r.post_id=m.source_post_id
                AND r.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'history',m.destination_post_id,jsonb_build_array(i.destination_board,i.destination_thread,
                encode(h.actor_hash,'hex'),h.request_at,h.automatic_identity,
                CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid::text END,
                h.automatic_browser,encode(h.automatic_prefix,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.posting_history h ON h.post_id=m.source_post_id
                AND h.board=i.source_board AND h.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'anonymous',m.destination_post_id,jsonb_build_array(encode(a.token_hash,'hex'),encode(a.password_proof,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.anonymous_posts a ON a.post_id=m.source_post_id
            JOIN post_secrets.anonymous_sessions session ON session.token_hash=a.token_hash AND session.expires_at>v_anonymous_at
            JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
            WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'email-ownership',m.destination_post_id,jsonb_build_array(o.identity_id,encode(o.password_proof,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN verification_private.post_ownership o ON o.post_id=m.source_post_id
            JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND o.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
            WHERE m.ticket_hash=p_ticket
        ), actual(kind,id,payload) AS (
            SELECT 'deletion',d.post_id,jsonb_build_array(d.password_hash) FROM post_secrets.deletion d
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=d.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'peer',o.thread_id,jsonb_build_array(o.peer) FROM post_secrets.op_peers o WHERE o.thread_id=i.destination_thread
            UNION ALL SELECT 'reply',r.post_id,jsonb_build_array(r.thread_id) FROM post_secrets.op_replies r
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=r.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'history',h.post_id,jsonb_build_array(h.board,h.thread_id,encode(h.actor_hash,'hex'),
                h.request_at,h.automatic_identity,h.registration_xid::text,h.automatic_browser,encode(h.automatic_prefix,'hex')) FROM post_secrets.posting_history h
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=h.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'anonymous',a.post_id,jsonb_build_array(encode(a.token_hash,'hex'),encode(a.password_proof,'hex'))
                FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=a.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'email-ownership',o.post_id,jsonb_build_array(o.identity_id,encode(o.password_proof,'hex'))
                FROM verification_private.post_ownership o JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=o.post_id WHERE m.ticket_hash=p_ticket
        ) SELECT 1 FROM ((SELECT * FROM expected EXCEPT ALL SELECT * FROM actual)
            UNION ALL (SELECT * FROM actual EXCEPT ALL SELECT * FROM expected)) different
    ) OR EXISTS(SELECT 1 FROM post_secrets.poster_contexts c JOIN post_secrets.thread_transfer_map m
        ON m.destination_post_id=c.post_id WHERE m.ticket_hash=p_ticket) THEN
        RAISE EXCEPTION 'Transfer evidence verification failed.' USING ERRCODE='P131R';
    END IF;
    UPDATE content.threads SET deleted=true WHERE board=i.source_board AND id=i.source_thread;
    INSERT INTO content.moderation_audit(account_id,board,target_id,action)
        VALUES(i.account_id,i.source_board,i.source_thread,'text-thread-transfer') RETURNING id INTO v_audit;
    INSERT INTO post_secrets.thread_transfer_results(request_id,account_id,source_board,source_thread,
        destination_board,destination_thread,post_count,audit_id)
        VALUES(i.request_id,i.account_id,i.source_board,i.source_thread,i.destination_board,i.destination_thread,v_count,v_audit);
    INSERT INTO post_secrets.thread_transfer_result_posts
        SELECT i.request_id,m.ordinal,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    PERFORM post_secrets.transfer_authority(i.session_hash,i.csrf_hash,i.idle_seconds,
        i.source_board,i.destination_board,true,false,i.credential_id);
    DELETE FROM post_secrets.thread_transfer_active WHERE ticket_hash=p_ticket AND transaction_id=pg_current_xact_id();
    DELETE FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    RETURN QUERY SELECT r.request_id::text,r.source_board,r.source_thread,r.destination_board,r.destination_thread,
        r.post_count::bigint,r.audit_id,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_results r JOIN post_secrets.thread_transfer_result_posts m ON m.request_id=r.request_id
        WHERE r.request_id=i.request_id ORDER BY m.ordinal;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets,staff_identity FROM board_thread_transfer_owner;

GRANT EXECUTE ON FUNCTION content.board_flag_label(text,text) TO board_restriction_owner;
GRANT SELECT(staff_notices),UPDATE(staff_notices) ON content.posts TO board_restriction_owner;
GRANT UPDATE(modified_at) ON content.threads TO board_restriction_owner;
GRANT CREATE ON SCHEMA restrictions,content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;

CREATE OR REPLACE FUNCTION restrictions.issue_post_restriction_with_notice(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_post bigint,
 p_global boolean,p_warning boolean,p_seconds bigint,p_public text,p_private text,p_notice boolean,p_custom text)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_actor bytea; v_automatic uuid; v_account bigint; v_id bigint; v_now timestamptz; v_charge bigint; v_thread bigint; v_notice text; v_custom_allowed boolean;
BEGIN
 IF restrictions.invoker() NOT IN('board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Restriction issuance is unavailable' USING ERRCODE='42501';
 END IF;
 IF p_notice IS NULL OR p_post IS NULL OR p_post<=0 OR p_global IS NULL OR p_warning IS NULL
    OR p_public IS NULL OR octet_length(p_public) NOT BETWEEN 1 AND 16384 OR p_private IS NULL
    OR octet_length(p_private)>32768 OR (p_warning AND p_seconds IS DISTINCT FROM 0)
    OR (NOT p_warning AND p_seconds IS NOT NULL AND p_seconds NOT BETWEEN 1 AND 863913600) THEN
  RAISE EXCEPTION 'Invalid restriction' USING ERRCODE='23514';
 END IF;
 v_account:=restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,true);
 IF p_notice THEN
  SELECT a.role IN('manager','admin') OR ('banmsg'=ANY(a.flags) AND staff_identity.has_board_access(a.id,''))
   INTO v_custom_allowed FROM staff_identity.accounts a WHERE a.id=v_account;
  v_notice:=CASE WHEN v_custom_allowed AND p_custom IS NOT NULL AND p_custom<>'' THEN p_custom
   WHEN p_warning THEN 'USER WAS WARNED FOR THIS POST' ELSE 'USER WAS BANNED FOR THIS POST' END;
  IF octet_length(v_notice)>4096 THEN RAISE EXCEPTION 'Post notice exceeds its limit' USING ERRCODE='23514'; END IF;
 END IF;
 SELECT h.actor_hash,h.automatic_identity INTO v_actor,v_automatic FROM post_secrets.posting_history h
    WHERE h.board=p_board AND h.post_id=p_post;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post identity is unavailable' USING ERRCODE='P134N'; END IF;
 -- Resolve the private actor before its gate; revalidate after parent locks.
 PERFORM content.lock_posting_actor(v_actor,false);
 PERFORM restrictions.gate();
 IF p_notice THEN
  PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE NOWAIT;
 ELSE
  PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR SHARE NOWAIT;
 END IF;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 SELECT p.thread_id INTO v_thread FROM content.posts p WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 IF p_notice THEN
  PERFORM t.id FROM content.threads t WHERE t.board=p_board AND t.id=v_thread AND NOT t.deleted
    AND t.archived_at IS NULL FOR UPDATE NOWAIT;
 ELSE
  PERFORM t.id FROM content.threads t WHERE t.board=p_board AND t.id=v_thread AND NOT t.deleted
    AND t.archived_at IS NULL FOR SHARE NOWAIT;
 END IF;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
 IF p_notice THEN
  PERFORM p.id FROM content.posts p JOIN post_secrets.posting_history h ON h.post_id=p.id AND h.board=p.board
    WHERE p.id=p_post AND p.board=p_board AND NOT p.deleted AND h.actor_hash=v_actor
       AND h.automatic_identity IS NOT DISTINCT FROM v_automatic FOR UPDATE OF p NOWAIT;
 ELSE
  PERFORM p.id FROM content.posts p JOIN post_secrets.posting_history h ON h.post_id=p.id AND h.board=p.board
    WHERE p.id=p_post AND p.board=p_board AND NOT p.deleted AND h.actor_hash=v_actor
       AND h.automatic_identity IS NOT DISTINCT FROM v_automatic FOR SHARE OF p NOWAIT;
 END IF;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post identity changed' USING ERRCODE='P134N'; END IF;
 IF p_notice AND (SELECT cardinality(staff_notices)>=16 FROM content.posts WHERE board=p_board AND id=p_post) THEN
  RAISE EXCEPTION 'Post notice capacity is exhausted' USING ERRCODE='P134C'; END IF;
 v_charge:=512+octet_length(p_public)::bigint+octet_length(p_private)::bigint;
 IF EXISTS(SELECT 1 FROM restrictions.capacity c WHERE c.singleton AND (c.used_bytes>c.byte_limit-v_charge
    OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans LIMIT 1000001) bounded)>=c.row_limit)) THEN
  RAISE EXCEPTION 'Restriction capacity is exhausted' USING ERRCODE='P134C';
 END IF;
 v_now:=date_trunc('second',clock_timestamp());
 INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,starts_at,ends_at,
    public_reason,private_reason,account_id,charged_bytes)
 VALUES(p_board,p_post,v_actor,v_automatic,p_global,p_warning,v_now,
    CASE WHEN p_warning THEN v_now WHEN p_seconds IS NULL THEN NULL ELSE v_now+make_interval(secs=>p_seconds::double precision) END,
    p_public,p_private,v_account,v_charge) RETURNING id INTO v_id;
 UPDATE restrictions.capacity SET used_bytes=used_bytes+v_charge WHERE singleton;
 INSERT INTO restrictions.audit(ban_id,action,account_id) VALUES(v_id,CASE WHEN p_warning THEN 'warning' ELSE 'ban' END,v_account);
 IF p_notice THEN
  UPDATE content.posts SET staff_notices=array_append(staff_notices,v_notice) WHERE board=p_board AND id=p_post AND NOT deleted;
  IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable' USING ERRCODE='P134N'; END IF;
  UPDATE content.threads SET modified_at=clock_timestamp() WHERE board=p_board AND id=v_thread;
  -- This manual endpoint has no combined post action. Future delete/move/archive
  -- callers must preserve the source exclusion before sharing this operation.
  PERFORM post_secrets.auto_clear_post_reports(p_board,p_post);
 END IF;
 -- Time-based validity is checked again after all contended content locks.
 PERFORM restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,true);
 RETURN v_id;
END $$;

CREATE OR REPLACE FUNCTION content.issue_post_restriction(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_post bigint,
 p_global boolean,p_warning boolean,p_seconds bigint,p_public text,p_private text)
RETURNS bigint LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT restrictions.issue_post_restriction_with_notice(p_session,p_csrf,p_idle,p_board,p_post,
  p_global,p_warning,p_seconds,p_public,p_private,false,NULL)
$$;
CREATE FUNCTION content.issue_post_restriction_notice(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_post bigint,
 p_global boolean,p_warning boolean,p_seconds bigint,p_public text,p_private text,p_notice boolean,p_custom text)
RETURNS bigint LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT restrictions.issue_post_restriction_with_notice(p_session,p_csrf,p_idle,p_board,p_post,
  p_global,p_warning,p_seconds,p_public,p_private,p_notice,p_custom)
$$;
CREATE FUNCTION content.post_notices_ready() RETURNS boolean LANGUAGE sql STABLE
SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT content.restrictions_ready()
 AND EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.posts'::regclass
  AND attname='staff_notices' AND atttypid='text[]'::regtype AND attnotnull AND NOT attisdropped)
 AND NOT EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth')
  AND has_column_privilege(r.oid,'content.posts','staff_notices','INSERT,UPDATE'))
 AND has_column_privilege('board_restriction_owner','content.posts','staff_notices','SELECT,UPDATE')
 AND has_column_privilege('board_thread_transfer_owner','content.posts','staff_notices','SELECT,INSERT')
$$;
REVOKE ALL ON FUNCTION restrictions.issue_post_restriction_with_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text),
 content.issue_post_restriction_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text),content.post_notices_ready() FROM PUBLIC;
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT p.oid::regprocedure AS identity,a.grantee FROM pg_proc p
 CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
 WHERE p.oid IN('restrictions.issue_post_restriction_with_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text)'::regprocedure,
  'content.issue_post_restriction_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text)'::regprocedure,
  'content.post_notices_ready()'::regprocedure) AND a.grantee NOT IN(0,p.proowner)
 LOOP EXECUTE format('REVOKE ALL ON FUNCTION %s FROM %I',r.identity,pg_get_userbyid(r.grantee)); END LOOP;
END $$;
GRANT EXECUTE ON FUNCTION content.issue_post_restriction_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text) TO board_staff;
GRANT EXECUTE ON FUNCTION content.post_notices_ready() TO board_public,board_staff,board_migrator;

CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions' OR c.oid IN('post_secrets.posting_history'::regclass,
    'admission.policy'::regclass,'admission.file_rules'::regclass,
    'admission.automatic_ban_exemptions'::regclass,'admission.dmca_warnings'::regclass,
    'admission.file_effect_capacity'::regclass,'content.posts'::regclass,'content.threads'::regclass)
 ), triggers AS (
  SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'post_secrets.posting_history'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='post_secrets.posting_history'::regclass AND t.tgname='source_restriction_guard' AND NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'content.reports'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='content.reports'::regclass AND t.tgname='source_report_restriction_guard' AND NOT t.tgisinternal
 ), funcs AS (
  SELECT p.* FROM pg_proc p WHERE p.proowner='board_restriction_owner'::regrole
    OR p.oid IN(SELECT t.tgfoid FROM triggers t)
    OR p.oid IN('content.lock_automatic_range_policy(boolean)'::regprocedure,
       'content.record_inserted_posting_history()'::regprocedure,
       'content.lock_report_network_policy()'::regprocedure,
       'content.report_network_rules(text,bigint,boolean)'::regprocedure,
       'post_secrets.report_network_activity(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
       'post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
       'post_secrets.assert_report_network(text,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,restrictions.acl(coalesce(r.relacl,acldefault(CASE WHEN r.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
       pg_get_expr(d.adbin,d.adrelid),restrictions.acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated)
       ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
    ORDER BY t.identity,t.tgname) FROM triggers t),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),pg_get_functiondef(p.oid),
      restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),
      has_function_privilege('board_restriction_owner',p.oid,'EXECUTE')) ORDER BY p.oid::regprocedure::text)
   FROM pg_proc p WHERE p.oid IN('staff_identity.has_board_access(bigint,text)'::regprocedure,
      'admission.resolve_file_effect_rule(text,bigint,bytea,bytea,boolean)'::regprocedure,
      'admission.file_effect_exempt(inet)'::regprocedure,
      'admission.file_effect_dmca(bytea)'::regprocedure,
      'admission.automatic_ban_exempt(inet)'::regprocedure,
      'admission.record_dmca_warning(bytea,timestamptz)'::regprocedure,
      'content.file_effect_upload_metadata(text,text,text,bigint,boolean)'::regprocedure,
      'content.prior_ban_upload_digest(text,text,text,bigint,boolean,boolean)'::regprocedure,
      'content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)'::regprocedure,
      'content.check_attachment_upload(text,text)'::regprocedure,
      'content.check_attachment_board_policy(text,bigint,bigint,integer,integer,text,text,bigint,boolean)'::regprocedure,
      'content.revalidate_staff_refusal(bytea,bytea,integer,text)'::regprocedure,
      'content.text_thread_transfer_ready()'::regprocedure,
      'content.lock_posting_actor(bytea,boolean)'::regprocedure,
      'post_secrets.lookup_automatic_identity(bytea,bigint)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
      'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),restrictions.acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
   FROM pg_namespace n WHERE n.nspname='restrictions'))::text,'UTF8'))
$$;

RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,content FROM board_restriction_owner;

CREATE OR REPLACE VIEW site_private.frontpage_eligible WITH(security_barrier=true) AS
 SELECT p.id,p.board,p.thread_id,left(b.title,120) AS board_title,
        left(p.subject,120) AS subject,left(p.comment || CASE WHEN cardinality(p.staff_notices)=0 THEN '' ELSE E'\n\n(' || array_to_string(p.staff_notices,E')\n\n(') || ')' END,300) AS excerpt,
        p.created_at,t.bumped_at,t.sticky,
        (SELECT count(*)::integer FROM content.posts r WHERE r.board=p.board AND r.thread_id=p.thread_id AND r.id<>p.thread_id AND NOT r.deleted) AS reply_count,
        (SELECT count(*)::integer FROM content.posts r JOIN content.visible_post_media rm ON rm.post_id=r.id AND NOT rm.file_deleted WHERE r.board=p.board AND r.thread_id=p.thread_id AND r.id<>p.thread_id AND NOT r.deleted) AS image_count,
        m.tim,m.bytes,m.thumbnail_width,m.thumbnail_height
 FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
 JOIN content.boards b ON b.slug=p.board
 LEFT JOIN content.visible_post_media m ON m.post_id=p.id AND NOT m.file_deleted
 WHERE NOT p.deleted AND NOT t.deleted AND t.archived_at IS NULL AND NOT b.staff_only
 AND p.board NOT IN ('b','f','qa') AND p.created_at<CURRENT_TIMESTAMP-interval '10 minutes'
 AND NOT p.image_spoiler AND NOT coalesce(m.spoiler,false)
 AND strpos(p.subject,'SPOILER<>')<>1 AND strpos(p.comment,'[spoiler]')=0
 AND strpos(coalesce(m.filename,''),'.pdf')=0 AND strpos(coalesce(m.filename,''),'.webm')=0
 AND NOT EXISTS(SELECT 1 FROM content.reports r WHERE r.board=p.board AND r.post_id=p.id);

SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.post_notices_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Post notices are not ready'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Post notices changed report profile authority'; END IF; END $$;

