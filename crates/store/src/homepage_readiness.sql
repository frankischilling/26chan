-- Exact PostgreSQL view projections preserve parentheses and casts.
WITH relations AS (
 SELECT c.*,n.nspname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE (n.nspname='site_private' AND c.relname IN ('frontpage_state','frontpage_cards','board_statistics','frontpage_eligible'))
 OR (n.nspname='content' AND c.relname IN ('homepage_cards','homepage_statistics','homepage_board_statistics'))
)
SELECT (SELECT count(*) FROM relations)=7
AND EXISTS(SELECT 1 FROM pg_namespace WHERE nspname='site_private'
 AND nspowner=(SELECT oid FROM pg_roles WHERE rolname='board_migrator'))
AND NOT EXISTS(SELECT 1 FROM pg_roles runtime WHERE runtime.rolname LIKE 'board\_%' ESCAPE '\'
 AND runtime.rolname<>'board_migrator' AND has_schema_privilege(runtime.oid,'site_private','USAGE,CREATE'))
AND NOT EXISTS(SELECT 1 FROM relations r CROSS JOIN LATERAL aclexplode(r.relacl) acl
 WHERE acl.grantee NOT IN ((SELECT oid FROM pg_roles WHERE rolname='board_migrator'),
 CASE WHEN r.nspname='content' THEN (SELECT oid FROM pg_roles WHERE rolname='board_public') ELSE 0::oid END)
 OR acl.grantee=0 OR (acl.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_public')
 AND (acl.privilege_type<>'SELECT' OR acl.is_grantable)))
AND NOT EXISTS(SELECT 1 FROM relations r JOIN pg_attribute a ON a.attrelid=r.oid
 CROSS JOIN LATERAL aclexplode(a.attacl) acl
 WHERE acl.grantee<>(SELECT oid FROM pg_roles WHERE rolname='board_migrator'))
AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.relowner<>(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
 OR (r.nspname='site_private' AND r.relname<>'frontpage_eligible' AND (r.relkind<>'r'
 OR has_table_privilege('board_public',r.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 OR has_any_column_privilege('board_public',r.oid,'SELECT,INSERT,UPDATE,REFERENCES')))
 OR ((r.nspname='content' OR r.relname='frontpage_eligible') AND (r.relkind<>'v'
 OR NOT COALESCE('security_barrier=true'=ANY(r.reloptions),false)
 OR COALESCE('security_invoker=true'=ANY(r.reloptions),false)
 OR (r.nspname='content' AND NOT has_table_privilege('board_public',r.oid,'SELECT'))
 OR (r.nspname='site_private' AND has_table_privilege('board_public',r.oid,'SELECT'))
 OR has_table_privilege('board_public',r.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))))
AND NOT EXISTS(SELECT 1 FROM (VALUES
 ('site_private','frontpage_eligible',$definition$SELECTp.id,p.board,p.thread_id,"left"(b.title,120)ASboard_title,"left"(p.subject,120)ASsubject,"left"(p.comment,300)ASexcerpt,p.created_at,t.bumped_at,t.sticky,(SELECTcount(*)::integerAScountFROMcontent.postsrWHEREr.board=p.boardANDr.thread_id=p.thread_idANDr.id<>p.thread_idANDNOTr.deleted)ASreply_count,(SELECTcount(*)::integerAScountFROMcontent.postsrJOINcontent.visible_post_mediarmONrm.post_id=r.idANDNOTrm.file_deletedWHEREr.board=p.boardANDr.thread_id=p.thread_idANDr.id<>p.thread_idANDNOTr.deleted)ASimage_count,m.tim,m.bytes,m.thumbnail_width,m.thumbnail_heightFROMcontent.postspJOINcontent.threadstONt.id=p.thread_idANDt.board=p.boardJOINcontent.boardsbONb.slug=p.boardLEFTJOINcontent.visible_post_mediamONm.post_id=p.idANDNOTm.file_deletedWHERENOTp.deletedANDNOTt.deletedANDt.archived_atISNULLANDNOTb.staff_onlyAND(p.board<>ALL(ARRAY['b'::text,'f'::text,'qa'::text]))ANDp.created_at<(CURRENT_TIMESTAMP-'00:10:00'::interval)ANDNOTp.image_spoilerANDNOTCOALESCE(m.spoiler,false)ANDstrpos(p.subject,'SPOILER<>'::text)<>1ANDstrpos(p.comment,'[spoiler]'::text)=0ANDstrpos(COALESCE(m.filename,''::text),'.pdf'::text)=0ANDstrpos(COALESCE(m.filename,''::text),'.webm'::text)=0ANDNOT(EXISTS(SELECT1FROMcontent.reportsrWHEREr.board=p.boardANDr.post_id=p.id));$definition$),
 ('content','homepage_cards',$definition$SELECTc.kind,c.slot,e.id,e.board,e.thread_id,e.board_title,e.subject,e.excerpt,e.created_at,e.reply_count,e.image_count,e.tim,e.bytes,e.thumbnail_width,e.thumbnail_heightFROMsite_private.frontpage_cardscJOINsite_private.frontpage_eligibleeONe.id=c.post_idWHERE(c.kind<>'images'::textORe.timISNOTNULL)AND(c.kind<>'popular'::textORe.id=e.thread_idANDe.reply_count>=15);$definition$),
 ('content','homepage_statistics',$definition$SELECTgeneration,generated_at,recorded_posts,active_content_bytes,active_sessionsFROMsite_private.frontpage_stateWHEREboard_slugs=(ARRAY(SELECTboards.slugFROMcontent.boardsWHERENOTboards.staff_onlyORDERBYboards.source_order,boards.slugLIMIT100));$definition$),
 ('content','homepage_board_statistics',$definition$SELECTs.board,s.generated_at,s.recorded_posts,s.last_post_id,s.active_threadsFROMsite_private.board_statisticssJOINcontent.boardsbONb.slug=s.boardWHERENOTb.staff_onlyAND(s.board=ANY(((SELECTfrontpage_state.board_slugsFROMsite_private.frontpage_state))::text[]));$definition$)
 ) expected(schema,name,definition)
 WHERE NOT EXISTS(SELECT 1 FROM relations r WHERE r.nspname=expected.schema AND r.relname=expected.name
 AND regexp_replace(pg_get_viewdef(r.oid,true),'[[:space:]]','','g')=expected.definition))
AND NOT EXISTS(SELECT 1 FROM (VALUES
 ('frontpage_state',$definition$CHECK (singleton)$definition$),
 ('frontpage_state',$definition$CHECK (generation >= 1 AND generation <= 2147483647)$definition$),
 ('frontpage_state',$definition$CHECK (cardinality(board_slugs) <= 100 AND array_position(board_slugs, NULL::text) IS NULL)$definition$),
 ('frontpage_state',$definition$CHECK (recorded_posts >= 0)$definition$),
 ('frontpage_state',$definition$CHECK (active_content_bytes >= 0)$definition$),
 ('frontpage_state',$definition$CHECK (active_sessions >= 0 AND active_sessions <= 1000000)$definition$),
 ('frontpage_state',$definition$PRIMARY KEY (singleton)$definition$),
 ('frontpage_cards',$definition$CHECK (kind = ANY (ARRAY['popular'::text, 'latest'::text, 'images'::text]))$definition$),
 ('frontpage_cards',$definition$CHECK (slot >= 1 AND slot <= 12)$definition$),
 ('frontpage_cards',$definition$PRIMARY KEY (kind, slot)$definition$),
 ('frontpage_cards',$definition$UNIQUE (kind, post_id)$definition$),
 ('frontpage_cards',$definition$FOREIGN KEY (post_id) REFERENCES content.posts(id) ON DELETE CASCADE$definition$),
 ('board_statistics',$definition$CHECK (slot >= 1 AND slot <= 64)$definition$),
 ('board_statistics',$definition$CHECK (recorded_posts >= 0)$definition$),
 ('board_statistics',$definition$CHECK (last_post_id >= 0)$definition$),
 ('board_statistics',$definition$CHECK (active_threads >= 0)$definition$),
 ('board_statistics',$definition$PRIMARY KEY (board, slot)$definition$),
 ('board_statistics',$definition$FOREIGN KEY (board) REFERENCES content.boards(slug) ON DELETE CASCADE$definition$)
 ) expected(name,definition)
 WHERE NOT EXISTS(SELECT 1 FROM pg_constraint c JOIN relations r ON r.oid=c.conrelid
 WHERE r.nspname='site_private' AND r.relname=expected.name AND c.convalidated AND NOT c.condeferrable
 AND regexp_replace(pg_get_constraintdef(c.oid,true),'[[:space:]]','','g')=regexp_replace(expected.definition,'[[:space:]]','','g')))
