-- Expose the source global1 warning-shortcut restriction to the scoped queue.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request authority drifted before queue readers'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA staff_identity TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE OR REPLACE FUNCTION staff_identity.janitor_request_queue(p_session bytea,p_csrf bytea,p_idle integer,p_board text,p_offset integer)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; rank integer; page jsonb; choices jsonb; counts jsonb; raw_count integer; over_budget boolean;
BEGIN
 IF restrictions.invoker() NOT IN('board_auth','board_migrator') THEN
  RAISE EXCEPTION 'Request queue is unavailable' USING ERRCODE='42501'; END IF;
 IF (p_board IS NOT NULL AND p_board !~ '^[a-z0-9]{1,10}$') OR p_offset IS NULL OR p_offset NOT BETWEEN 0 AND 100000 THEN
  RAISE EXCEPTION 'Invalid request queue page' USING ERRCODE='23514'; END IF;
 account:=restrictions.message_authority(p_session,p_csrf,p_idle);
 SELECT restrictions.template_rank(a.role) INTO rank FROM staff_identity.accounts a WHERE a.id=account;
 IF rank<10 OR (p_board IS NOT NULL AND NOT staff_identity.has_board_access(account,p_board)) THEN
  RAISE EXCEPTION 'Request queue scope is unavailable' USING ERRCODE='P134A'; END IF;
 PERFORM restrictions.gate(false);
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.requests LIMIT 100001) bounded)>100000
  OR (SELECT count(*) FROM (SELECT 1 FROM restrictions.templates LIMIT 129) bounded)>128 THEN
  RAISE EXCEPTION 'Request queue inventory exceeds its bound' USING ERRCODE='P134C'; END IF;
 -- Source prepends priority rules while traversing its initial order. Reverse
 -- only that subset. Stable IDs resolve the source database's unspecified ties.
 WITH ranked AS (
  SELECT r.id,r.board,r.post_id,r.template_id,coalesce(t.name,r.template_name) AS template_name,
   r.warning,r.account_id,r.created_at,r.payload,r.charged_bytes,
   coalesce(t.source_rule IN('global1','hm6','s6'),false) AS priority,
   coalesce(t.enabled AND restrictions.template_rank(t.minimum_role)<=rank
    AND t.request_save_policy IN('none','json_only') AND t.request_blacklist_policy IN('none','image','rejectimage')
    AND (r.warning OR t.request_blacklist_policy='none' OR r.upload_observation IN('known','absent')),false) AS can_accept,
   row_number() OVER(ORDER BY CASE WHEN p_board IS NOT NULL THEN r.template_id END,r.created_at,r.id) AS position
  FROM restrictions.requests r LEFT JOIN restrictions.templates t ON t.id=r.template_id
  WHERE (CASE WHEN p_board IS NULL THEN r.board<>'test' ELSE r.board=p_board END)
   AND staff_identity.has_board_access(account,r.board)
 ), selected AS (
  SELECT *,CASE WHEN priority THEN -position ELSE position END AS item_order FROM ranked
  ORDER BY priority DESC,CASE WHEN priority THEN -position ELSE position END OFFSET p_offset LIMIT 26
 ), numbered AS (
  SELECT *,row_number() OVER(ORDER BY priority DESC,item_order) AS item_number,
   sum(charged_bytes) OVER(ORDER BY priority DESC,item_order) AS page_bytes FROM selected
 )
 SELECT count(*),coalesce(bool_or(page_bytes>16777216) FILTER(WHERE item_number<=25),false),
  coalesce(jsonb_agg(jsonb_build_object('id',id,'board',board,'post_id',post_id,'template_id',template_id,
   'template_name',template_name,'warning',warning,'account_id',account_id,'created_at',created_at,
   'can_accept',can_accept,'payload',CASE WHEN page_bytes<=16777216 THEN payload::text END)
   ORDER BY priority DESC,item_order) FILTER(WHERE item_number<=25),'[]') INTO raw_count,over_budget,page FROM numbered;
 IF over_budget THEN RAISE EXCEPTION 'Request page exceeds its byte budget' USING ERRCODE='P134C'; END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',t.id,'name',t.name,'global',t.global,'warning',t.warning,
  'seconds',t.seconds,'no_warn',coalesce(t.source_rule='global1',false),'available',coalesce(t.request_save_policy IN('none','json_only')
   AND t.request_blacklist_policy IN('none','image','rejectimage'),false)) ORDER BY t.id),'[]') INTO choices
  FROM restrictions.templates t WHERE t.enabled AND restrictions.template_rank(t.minimum_role)<=rank;
 SELECT coalesce(jsonb_agg(jsonb_build_object('board',b.board,'count',b.count) ORDER BY b.board),'[]') INTO counts
  FROM (SELECT r.board,count(*) AS count FROM restrictions.requests r
   WHERE staff_identity.has_board_access(account,r.board) GROUP BY r.board) b;
 PERFORM restrictions.message_authority(p_session,p_csrf,p_idle);
 RETURN jsonb_build_object('requests',page,'templates',choices,'counts',counts,'offset',p_offset,
  'previous_offset',greatest(0,p_offset-25),'next_offset',CASE WHEN raw_count=26 THEN p_offset+25 ELSE 0 END);
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA staff_identity FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true
 OR content.report_snapshots_ready() IS DISTINCT FROM true
 OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request shortcut metadata changed authority'; END IF; END $$;
RESET ROLE;
