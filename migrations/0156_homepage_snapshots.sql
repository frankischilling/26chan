-- Bounded operator refresh; public reads never retain copies of post bodies.
CREATE INDEX reports_frontpage_target ON content.reports(board,post_id);
CREATE INDEX posts_frontpage_recent ON content.posts(board,created_at DESC,id DESC) WHERE NOT deleted;
CREATE TABLE site_private.frontpage_state (
    singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
    generation bigint NOT NULL CHECK(generation BETWEEN 1 AND 2147483647),
    generated_at timestamptz NOT NULL,
    board_slugs text[] NOT NULL CHECK(cardinality(board_slugs)<=100 AND array_position(board_slugs,NULL) IS NULL),
    recorded_posts bigint NOT NULL CHECK(recorded_posts>=0),
    active_content_bytes bigint NOT NULL CHECK(active_content_bytes>=0),
    active_sessions bigint NOT NULL CHECK(active_sessions BETWEEN 0 AND 1000000)
);
CREATE TABLE site_private.frontpage_cards (
    kind text NOT NULL CHECK(kind IN ('popular','latest','images')),
    slot smallint NOT NULL CHECK(slot BETWEEN 1 AND 12),
    post_id bigint NOT NULL REFERENCES content.posts(id) ON DELETE CASCADE,
    PRIMARY KEY(kind,slot), UNIQUE(kind,post_id)
);
CREATE TABLE site_private.board_statistics (
    board text NOT NULL REFERENCES content.boards(slug) ON DELETE CASCADE,
    slot smallint NOT NULL CHECK(slot BETWEEN 1 AND 64),
    generated_at timestamptz NOT NULL,
    recorded_posts bigint NOT NULL CHECK(recorded_posts>=0),
    last_post_id bigint NOT NULL CHECK(last_post_id>=0),
    active_threads bigint NOT NULL CHECK(active_threads>=0),
    PRIMARY KEY(board,slot)
);
REVOKE ALL ON site_private.frontpage_state,site_private.frontpage_cards,site_private.board_statistics FROM PUBLIC,board_public,board_staff,board_auth;

-- Literal source exclusions plus current deletion, privacy and spoiler state.
-- Only bounded excerpts and public media identities leave this projection.
CREATE VIEW site_private.frontpage_eligible WITH(security_barrier=true) AS
 SELECT p.id,p.board,p.thread_id,left(b.title,120) AS board_title,
        left(p.subject,120) AS subject,left(p.comment,300) AS excerpt,
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
REVOKE ALL ON site_private.frontpage_eligible FROM PUBLIC,board_public,board_staff,board_auth;
CREATE VIEW content.homepage_cards WITH(security_barrier=true) AS
 SELECT c.kind,c.slot,e.id,e.board,e.thread_id,e.board_title,e.subject,e.excerpt,e.created_at,
        e.reply_count,e.image_count,e.tim,e.bytes,e.thumbnail_width,e.thumbnail_height
 FROM site_private.frontpage_cards c JOIN site_private.frontpage_eligible e ON e.id=c.post_id
 WHERE (c.kind<>'images' OR e.tim IS NOT NULL)
 AND (c.kind<>'popular' OR (e.id=e.thread_id AND e.reply_count>=15));
CREATE VIEW content.homepage_statistics WITH(security_barrier=true) AS
 SELECT generation,generated_at,recorded_posts,active_content_bytes,active_sessions
 FROM site_private.frontpage_state
 WHERE board_slugs=ARRAY(SELECT slug FROM content.boards WHERE NOT staff_only ORDER BY source_order,slug LIMIT 100);
CREATE VIEW content.homepage_board_statistics WITH(security_barrier=true) AS
 SELECT s.board,s.generated_at,s.recorded_posts,s.last_post_id,s.active_threads
 FROM site_private.board_statistics s JOIN content.boards b ON b.slug=s.board
 WHERE NOT b.staff_only AND s.board=ANY((SELECT board_slugs FROM site_private.frontpage_state)::text[]);
REVOKE ALL ON content.homepage_cards,content.homepage_statistics,content.homepage_board_statistics FROM PUBLIC,board_public,board_staff,board_auth;
GRANT SELECT ON content.homepage_cards,content.homepage_statistics,content.homepage_board_statistics TO board_public;
