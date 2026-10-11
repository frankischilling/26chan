-- Source cpban: global OP/distinct-address bans, then thread deletion.
-- A private deferred promise makes the application deletion atomic with bans.
DO $thread_ban_preflight$
BEGIN
 EXECUTE 'SET LOCAL ROLE board_restriction_owner';
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restrictions are not ready before thread batches'; END IF;
 PERFORM singleton FROM restrictions.capacity WHERE singleton FOR UPDATE;
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 EXECUTE 'RESET ROLE';
 EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer is not ready before thread batches'; END IF;
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 EXECUTE 'RESET ROLE';
END $thread_ban_preflight$;

GRANT SELECT(flags) ON staff_identity.accounts TO board_restriction_owner;
GRANT SELECT(sticky) ON content.threads TO board_restriction_owner;
GRANT CREATE ON SCHEMA restrictions,content TO board_restriction_owner;
SET ROLE board_restriction_owner;

-- At most one promise per transaction. The exclusive restriction gate bounds
-- concurrent promises; successful commits remove the only stored credentials.
CREATE TABLE restrictions.pending_thread_batches (
 registration_xid xid8 PRIMARY KEY,
 board text NOT NULL REFERENCES content.boards(slug),
 thread_id bigint NOT NULL CHECK(thread_id>0),
 account_id bigint NOT NULL CHECK(account_id>0),
 session_hash bytea NOT NULL CHECK(octet_length(session_hash)=32),
 csrf_hash bytea NOT NULL CHECK(octet_length(csrf_hash)=32),
 idle_seconds integer NOT NULL CHECK(idle_seconds BETWEEN 60 AND 3600)
);
REVOKE ALL ON restrictions.pending_thread_batches FROM PUBLIC;

CREATE FUNCTION restrictions.thread_batch_authority(p_session bytea,p_csrf bytea,p_idle integer,p_board text)
RETURNS bigint LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_account bigint;
BEGIN
 v_account:=restrictions.staff_authority(p_session,p_csrf,p_idle,p_board,true);
 -- adminvalid has a moderator floor; the form additionally requires manager
 -- rank or has_flag('threadban') with the source's no-board scope.
 IF NOT EXISTS(SELECT 1 FROM staff_identity.accounts a WHERE a.id=v_account
    AND (a.role IN('manager','admin') OR ('threadban'=ANY(a.flags)
       AND staff_identity.has_board_access(a.id,'')))) THEN
  RAISE EXCEPTION 'Thread ban authority is unavailable' USING ERRCODE='P134A'; END IF;
 RETURN v_account;
END $$;

CREATE FUNCTION restrictions.thread_batch_subjects(p_board text,p_thread bigint)
RETURNS TABLE(post_id bigint,actor bytea) LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_op bytea; v_posts bigint[]; v_actors bytea[];
BEGIN
 -- Capture one bounded statement snapshot. Separate count/group statements
 -- would allow intervening replies to enlarge the grouped input without limit.
 SELECT array_agg(p.id ORDER BY p.id),array_agg(h.actor_hash ORDER BY p.id) INTO v_posts,v_actors
  FROM (SELECT id,board FROM content.posts WHERE board=p_board AND thread_id=p_thread
        AND NOT deleted ORDER BY id LIMIT 10001) p
  LEFT JOIN post_secrets.posting_history h ON h.board=p.board AND h.post_id=p.id;
 IF cardinality(v_posts)>10000 THEN
  RAISE EXCEPTION 'Thread batch exceeds its post bound' USING ERRCODE='P134C'; END IF;
 v_op:=v_actors[array_position(v_posts,p_thread)];
 IF v_op IS NULL OR array_position(v_actors,NULL) IS NOT NULL THEN
  RAISE EXCEPTION 'Thread identity is unavailable' USING ERRCODE='P134N'; END IF;
 -- SQL GROUP BY in the source chooses an unspecified representative. Use the
 -- oldest live post explicitly, while always using the OP for its own address.
 RETURN QUERY SELECT CASE WHEN a.actor=v_op THEN p_thread ELSE min(a.no) END,a.actor
  FROM unnest(v_posts,v_actors) a(no,actor)
  GROUP BY a.actor ORDER BY a.actor LIMIT 1001;
END $$;

CREATE FUNCTION restrictions.complete_thread_batch() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF NEW.registration_xid IS DISTINCT FROM pg_current_xact_id()
    OR NOT EXISTS(SELECT 1 FROM content.threads t WHERE t.board=NEW.board AND t.id=NEW.thread_id AND t.deleted)
    OR EXISTS(SELECT 1 FROM content.posts p WHERE p.board=NEW.board AND p.thread_id=NEW.thread_id AND NOT p.deleted) THEN
  RAISE EXCEPTION 'Thread batch deletion is incomplete' USING ERRCODE='P136D'; END IF;
 IF restrictions.thread_batch_authority(NEW.session_hash,NEW.csrf_hash,NEW.idle_seconds,NEW.board)
      IS DISTINCT FROM NEW.account_id THEN
  RAISE EXCEPTION 'Thread batch authority changed' USING ERRCODE='P134A'; END IF;
 DELETE FROM restrictions.pending_thread_batches WHERE registration_xid=NEW.registration_xid;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER complete_source_thread_batch
 AFTER INSERT ON restrictions.pending_thread_batches DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW EXECUTE FUNCTION restrictions.complete_thread_batch();

CREATE FUNCTION content.issue_thread_restrictions(p_session bytea,p_csrf bytea,p_idle integer,
 p_board text,p_thread bigint,p_op_reason text,p_reply_reason text,p_op_days integer,p_reply_days integer)
RETURNS TABLE(ban_count integer,account_id bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_account bigint; v_actors bytea[]; v_posts bigint[]; v_stripes integer[]; v_actor bytea;
 v_count integer; v_i integer; v_days integer; v_reason text; v_private text; v_charge bigint;
 v_total bigint:=0; v_now timestamptz; v_id bigint; v_sticky boolean;
BEGIN
 IF restrictions.invoker() NOT IN('board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Thread batch is unavailable' USING ERRCODE='42501'; END IF;
 IF p_thread IS NULL OR p_thread<=0 OR p_op_reason IS NULL OR p_reply_reason IS NULL
    OR p_op_reason IN('','0') OR p_reply_reason IN('','0')
    OR octet_length(p_op_reason)>16384 OR octet_length(p_reply_reason)>16384
    OR p_op_days IS NULL OR p_reply_days IS NULL OR p_op_days NOT BETWEEN 0 AND 9999
    OR p_reply_days NOT BETWEEN 0 AND 9999 THEN
  RAISE EXCEPTION 'Invalid thread batch' USING ERRCODE='23514'; END IF;
 v_account:=restrictions.thread_batch_authority(p_session,p_csrf,p_idle,p_board);
 SELECT array_agg(s.actor ORDER BY s.post_id<>p_thread,s.actor),array_agg(s.post_id ORDER BY s.post_id<>p_thread,s.actor) INTO v_actors,v_posts
  FROM restrictions.thread_batch_subjects(p_board,p_thread) s;
 IF cardinality(v_actors) NOT BETWEEN 1 AND 1000 THEN
  RAISE EXCEPTION 'Thread batch exceeds its identity bound' USING ERRCODE='P134C'; END IF;
 SELECT array_agg(DISTINCT (get_byte(a,0)*256+get_byte(a,1))%4096) INTO v_stripes FROM unnest(v_actors) a;
 -- Canonical stripe order matters for batches with colliding actor digests.
 FOR v_actor IN SELECT a FROM unnest(v_actors) a ORDER BY (get_byte(a,0)*256+get_byte(a,1))%4096,a LOOP
  PERFORM content.lock_posting_actor(v_actor,false);
 END LOOP;
 PERFORM restrictions.gate();
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Thread is unavailable' USING ERRCODE='P134N'; END IF;
 SELECT t.sticky INTO v_sticky FROM content.threads t WHERE t.board=p_board AND t.id=p_thread
  AND NOT t.deleted AND t.archived_at IS NULL FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Thread is unavailable' USING ERRCODE='P134N'; END IF;
 -- Preserve ordinary sticky-OP deletion authority. A rejected deletion must
 -- not leave the earlier bans committed as the source's unchecked RPC can.
 IF v_sticky AND NOT EXISTS(SELECT 1 FROM staff_identity.accounts a WHERE a.id=v_account AND a.role='admin') THEN
  RAISE EXCEPTION 'Thread deletion authority is unavailable' USING ERRCODE='P134A'; END IF;
 PERFORM p.id FROM content.posts p WHERE p.board=p_board AND p.thread_id=p_thread
  AND NOT p.deleted ORDER BY p.id LIMIT 10001 FOR UPDATE NOWAIT;
 SELECT array_agg(s.actor ORDER BY s.post_id<>p_thread,s.actor),array_agg(s.post_id ORDER BY s.post_id<>p_thread,s.actor) INTO v_actors,v_posts
  FROM restrictions.thread_batch_subjects(p_board,p_thread) s;
 v_count:=cardinality(v_actors);
 IF v_count NOT BETWEEN 1 AND 1000 THEN
  RAISE EXCEPTION 'Thread batch exceeds its identity bound' USING ERRCODE='P134C'; END IF;
 IF EXISTS(SELECT 1 FROM unnest(v_actors) a WHERE NOT ((get_byte(a,0)*256+get_byte(a,1))%4096=ANY(v_stripes))) THEN
  -- A reply arrived during capture. Never acquire a new actor stripe while
  -- holding the restriction gate; roll back and recapture in a fresh attempt.
  RAISE EXCEPTION 'Thread batch needs a new capture' USING ERRCODE='P136R'; END IF;
 v_private:='Thread Ban No.'||p_thread;
 FOR v_i IN 1..v_count LOOP
  v_reason:=CASE WHEN v_posts[v_i]=p_thread THEN p_op_reason ELSE p_reply_reason END;
  v_total:=v_total+512+octet_length(v_reason)+octet_length(v_private);
 END LOOP;
 IF EXISTS(SELECT 1 FROM restrictions.capacity c WHERE c.singleton AND (c.used_bytes>c.byte_limit-v_total
    OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans LIMIT 1000001) bounded)>c.row_limit-v_count)) THEN
  RAISE EXCEPTION 'Restriction capacity is exhausted' USING ERRCODE='P134C'; END IF;
 v_now:=date_trunc('second',clock_timestamp());
 FOR v_i IN 1..v_count LOOP
  v_reason:=CASE WHEN v_posts[v_i]=p_thread THEN p_op_reason ELSE p_reply_reason END;
  v_days:=CASE WHEN v_posts[v_i]=p_thread THEN p_op_days ELSE p_reply_days END;
  v_charge:=512+octet_length(v_reason)+octet_length(v_private);
  -- ban_post writes host and Pass, but no UserPwd password. No trusted Pass
  -- provider exists here; automatic posting equality must not replace it.
  INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,
    public_reason,private_reason,account_id,charged_bytes)
  VALUES(p_board,v_posts[v_i],v_actors[v_i],true,v_days=0,v_now,v_now+make_interval(secs=>v_days::double precision*86400),
    v_reason,v_private,v_account,v_charge) RETURNING id INTO v_id;
  INSERT INTO restrictions.audit(ban_id,action,account_id)
   VALUES(v_id,CASE WHEN v_days=0 THEN 'warning' ELSE 'ban' END,v_account);
 END LOOP;
 UPDATE restrictions.capacity SET used_bytes=used_bytes+v_total WHERE singleton;
 INSERT INTO restrictions.pending_thread_batches VALUES(pg_current_xact_id(),p_board,p_thread,v_account,p_session,p_csrf,p_idle);
 PERFORM restrictions.thread_batch_authority(p_session,p_csrf,p_idle,p_board);
 RETURN QUERY SELECT v_count,v_account;
END $$;
REVOKE ALL ON FUNCTION restrictions.thread_batch_authority(bytea,bytea,integer,text),
 restrictions.thread_batch_subjects(text,bigint),restrictions.complete_thread_batch(),
 content.issue_thread_restrictions(bytea,bytea,integer,text,bigint,text,text,integer,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.issue_thread_restrictions(bytea,bytea,integer,text,bigint,text,text,integer,integer)
 TO board_staff,board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,content FROM board_restriction_owner;

-- The additional narrow SELECT grants change reviewed transfer dependencies.
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer did not qualify after thread batches'; END IF; END $$;
RESET ROLE;
SET ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Thread batch catalog did not qualify'; END IF; END $$;
RESET ROLE;
