-- Opt-in, private report-category catalog substrate only. No production rows,
-- active revision, target/admission changes, or runtime reader is installed.
-- These are rewrite safety bounds, not recovered source limits: v1 envelopes,
-- at most 4096 rows, 8 MiB normalized JSONB text, 256-byte board strings,
-- 65536-byte exclusion strings, 4096-byte titles, and 64 retained revisions.
-- The offline loader separately bounds the original input file to 8 MiB.
-- SQL NULL and empty strings remain distinct; ordinal is the supplied order,
-- not a guessed source collation or an invented ID/title tie-breaker.
GRANT CREATE ON SCHEMA content,post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;

CREATE TABLE post_secrets.report_catalog_gate (
    singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton)
);
INSERT INTO post_secrets.report_catalog_gate(singleton) VALUES(true);
CREATE TABLE post_secrets.report_catalog_versions (
    revision bigint PRIMARY KEY CHECK(revision BETWEEN 1 AND 64),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    category_count integer NOT NULL CHECK(category_count BETWEEN 0 AND 4096)
);
CREATE TABLE post_secrets.report_catalog_rows (
    revision bigint NOT NULL REFERENCES post_secrets.report_catalog_versions(revision),
    ordinal integer NOT NULL CHECK(ordinal BETWEEN 1 AND 4096),
    id bigint NOT NULL CHECK(id>0),
    board text CHECK(octet_length(board)<=256),
    op_only boolean NOT NULL,
    reply_only boolean NOT NULL,
    image_only boolean NOT NULL,
    exclude_boards text CHECK(octet_length(exclude_boards)<=65536),
    title text NOT NULL CHECK(octet_length(title)<=4096),
    weight double precision NOT NULL CHECK(weight NOT IN
        ('Infinity'::double precision,'-Infinity'::double precision,'NaN'::double precision)),
    filtered bigint NOT NULL,
    PRIMARY KEY(revision,ordinal),
    UNIQUE(revision,id)
);
-- The migrator's owner membership is SET-only: ordinary migrator sessions
-- have no table privileges, including read, truncate, update, and delete.
-- There is no sequence or eviction API; each successful import adds one
-- immutable revision, even for an empty or previously imported catalog.
REVOKE ALL ON post_secrets.report_catalog_gate,post_secrets.report_catalog_versions,
    post_secrets.report_catalog_rows FROM PUBLIC,board_migrator,board_public,board_staff,board_auth;

CREATE FUNCTION content.read_report_catalog(p_revision bigint) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp SET extra_float_digits=3 AS $$
DECLARE v_invoker text; v_result jsonb;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker<>'board_migrator' THEN
        RAISE EXCEPTION 'Report catalog access is unavailable.' USING ERRCODE='42501';
    END IF;
    IF p_revision IS NULL OR p_revision NOT BETWEEN 1 AND 64 THEN
        RAISE EXCEPTION 'Invalid report catalog revision.' USING ERRCODE='22023';
    END IF;
    PERFORM 1 FROM post_secrets.report_catalog_versions WHERE revision=p_revision;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Report catalog revision not found.' USING ERRCODE='P0002';
    END IF;
    SELECT jsonb_build_object('version',1,'categories',coalesce(jsonb_agg(
        jsonb_build_object('id',r.id,'board',r.board,'op_only',r.op_only,
            'reply_only',r.reply_only,'image_only',r.image_only,
            'exclude_boards',r.exclude_boards,'title',r.title,
            'weight',r.weight,'filtered',r.filtered) ORDER BY r.ordinal),'[]'::jsonb))
        INTO v_result FROM post_secrets.report_catalog_rows r WHERE r.revision=p_revision;
    IF octet_length(v_result::text)>8388608 THEN
        RAISE EXCEPTION 'Report catalog capacity is unavailable.' USING ERRCODE='P0098';
    END IF;
    RETURN v_result;
END $$;

CREATE FUNCTION content.import_report_catalog(p_catalog jsonb) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp SET extra_float_digits=3 AS $$
DECLARE v_invoker text; v_revision bigint; v_row jsonb; v_ordinal bigint;
    v_count integer; v_id bigint; v_filtered bigint; v_weight double precision;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker<>'board_migrator' THEN
        RAISE EXCEPTION 'Report catalog access is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report catalog import requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF p_catalog IS NULL OR jsonb_typeof(p_catalog)<>'object'
        OR octet_length(p_catalog::text)>8388608 THEN
        RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
    END IF;
    IF NOT p_catalog ?& ARRAY['version','categories']
        OR (SELECT count(*) FROM jsonb_object_keys(p_catalog))<>2
        OR p_catalog->'version'<>'1'::jsonb
        OR jsonb_typeof(p_catalog->'categories')<>'array' THEN
        RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
    END IF;
    v_count:=jsonb_array_length(p_catalog->'categories');
    IF v_count>4096 THEN
        RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
    END IF;
    -- Only this independent gate is locked. No board/admission/session locks.
    -- It serializes inactive imports, not activation. Any future activation
    -- must integrate consumers and serialize with the report admission gate.
    PERFORM singleton FROM post_secrets.report_catalog_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Report catalog capacity is unavailable.' USING ERRCODE='P0098';
    END IF;
    -- Read Committed gives this statement a fresh snapshot after a gate wait.
    SELECT coalesce(max(revision),0)+1 INTO v_revision FROM post_secrets.report_catalog_versions;
    IF v_revision>64 THEN
        RAISE EXCEPTION 'Report catalog capacity is exhausted.' USING ERRCODE='P0098';
    END IF;
    INSERT INTO post_secrets.report_catalog_versions(revision,category_count) VALUES(v_revision,v_count);
    FOR v_row,v_ordinal IN SELECT value,ordinality FROM jsonb_array_elements(p_catalog->'categories') WITH ORDINALITY LOOP
        IF jsonb_typeof(v_row)<>'object' THEN
            RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
        END IF;
        IF NOT v_row ?& ARRAY['id','board','op_only','reply_only','image_only','exclude_boards','title','weight','filtered']
            OR (SELECT count(*) FROM jsonb_object_keys(v_row))<>9 THEN
            RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
        END IF;
        IF jsonb_typeof(v_row->'id')<>'number'
            OR jsonb_typeof(v_row->'filtered')<>'number'
            OR jsonb_typeof(v_row->'weight')<>'number'
            OR jsonb_typeof(v_row->'board') NOT IN ('string','null')
            OR jsonb_typeof(v_row->'exclude_boards') NOT IN ('string','null')
            OR jsonb_typeof(v_row->'title')<>'string'
            OR jsonb_typeof(v_row->'op_only')<>'boolean'
            OR jsonb_typeof(v_row->'reply_only')<>'boolean'
            OR jsonb_typeof(v_row->'image_only')<>'boolean' THEN
            RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
        END IF;
        -- JSONB has already normalized exponent notation and duplicate object
        -- keys. The offline raw JSON parser additionally rejects duplicate keys.
        -- Numeric strings, fractions, and out-of-range signed integers fail.
        IF octet_length(v_row->>'id')>20 OR octet_length(v_row->>'filtered')>20
            OR (v_row->>'id') !~ '^-?(0|[1-9][0-9]*)$'
            OR (v_row->>'filtered') !~ '^-?(0|[1-9][0-9]*)$'
            OR octet_length(v_row->>'board')>256
            OR octet_length(v_row->>'exclude_boards')>65536
            OR octet_length(v_row->>'title')>4096 THEN
            RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
        END IF;
        v_id:=(v_row->>'id')::bigint;
        v_filtered:=(v_row->>'filtered')::bigint;
        v_weight:=(v_row->>'weight')::double precision;
        IF v_id<=0 OR v_weight IN ('Infinity'::double precision,'-Infinity'::double precision,'NaN'::double precision) THEN
            RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
        END IF;
        INSERT INTO post_secrets.report_catalog_rows
            (revision,ordinal,id,board,op_only,reply_only,image_only,exclude_boards,title,weight,filtered)
            VALUES(v_revision,v_ordinal::integer,v_id,v_row->>'board',
                (v_row->>'op_only')::boolean,(v_row->>'reply_only')::boolean,
                (v_row->>'image_only')::boolean,v_row->>'exclude_boards',
                v_row->>'title',v_weight,v_filtered);
    END LOOP;
    -- Float decoding can change the normalized JSON size. Prove the exact
    -- bounded readback is available before admitting this immutable revision.
    PERFORM content.read_report_catalog(v_revision);
    RETURN v_revision;
EXCEPTION WHEN data_exception OR unique_violation THEN
    -- A failed import rolls back the header and every row, including duplicates
    -- discovered late in the array. No partial revision consumes capacity.
    RAISE EXCEPTION 'Invalid report catalog.' USING ERRCODE='22023';
END $$;

REVOKE ALL ON FUNCTION content.import_report_catalog(jsonb),content.read_report_catalog(bigint)
    FROM PUBLIC,board_public,board_staff,board_auth;
GRANT EXECUTE ON FUNCTION content.import_report_catalog(jsonb),content.read_report_catalog(bigint)
    TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_admission_owner;
