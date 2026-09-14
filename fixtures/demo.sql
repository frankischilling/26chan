-- Synthetic fixtures. No production content or credentials.
-- Created after migrations, so source overrides must be explicit here too.
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_max_lines,comment_spoiler_cleanup,require_subject)
VALUES ('qst','Quests','Synthetic required-subject posting fixture.',4000,100,75,100,10,100,true,true)
ON CONFLICT (slug) DO NOTHING;

INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,text_only)
VALUES ('news','News','Synthetic source text-only posting fixture.',4000,100,75,100,10,true)
ON CONFLICT (slug) DO NOTHING;

INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_code_spacing,comment_max_lines,comment_spoiler_cleanup)
VALUES ('test','Test board','A place to test text posts and replies.',4000,100,75,100,10,true,100,true),
       ('limit','Small limits','Synthetic concurrency tests: three replies per thread.',1000,3,2,100,10,false,70,false)
ON CONFLICT (slug) DO NOTHING;

INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES ('demo','Paper craft','Discuss paper models, folding, and works in progress.',4000,100,75,100,10)
ON CONFLICT (slug) DO NOTHING;
-- Deliberately separate HTTP and source clocks in this immutable demo row.
INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,http_modified_at,reply_count)
VALUES (1000001,'demo','2026-09-08 12:00:00+00','2026-09-08 12:05:00+00','2026-09-08 12:05:00+00','2026-09-08 12:06:00+00',1)
ON CONFLICT (id) DO NOTHING;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES (1000001,'demo',1000001,'Anonymous','What are you making?',E'Share your latest paper project.\n>start with a single sheet\n[spoiler]Mine is another crane.[/spoiler]','2026-09-08 12:00:00+00'),
       (1000002,'demo',1000001,'Anonymous','',E'>>1000001\nA small paper lighthouse. Still working on the roof.','2026-09-08 12:05:00+00')
ON CONFLICT (id) DO NOTHING;
SELECT setval('content.post_number', GREATEST((SELECT last_value FROM content.post_number), (SELECT max(id) FROM content.posts)));
-- These two immutable historical rows match the format-zero visual fixtures.
-- Newly submitted /demo/ posts still receive the trigger's disabled-policy stamp.
UPDATE content.posts SET comment_format=0 WHERE board='demo' AND id IN (1000001,1000002);

-- Active source OP_MARKUP overrides, including already-seeded boards.
UPDATE content.boards SET op_markup=true WHERE slug IN ('qst','test');
