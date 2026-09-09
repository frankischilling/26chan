-- Synthetic fixtures. No production content or credentials.
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES ('test','Test board','A place to test text posts and replies.',4000,100,75,100,10),
       ('limit','Small limits','Synthetic concurrency tests: three replies per thread.',1000,3,2,100,10)
ON CONFLICT (slug) DO NOTHING;

INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES ('demo','Paper craft','Discuss paper models, folding, and works in progress.',4000,100,75,100,10)
ON CONFLICT (slug) DO NOTHING;
INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,reply_count)
VALUES (1000001,'demo','2026-09-08 12:00:00+00','2026-09-08 12:05:00+00','2026-09-08 12:05:00+00',1)
ON CONFLICT (id) DO NOTHING;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES (1000001,'demo',1000001,'Anonymous','What are you making?',E'Share your latest paper project.\n>start with a single sheet\n[spoiler]Mine is another crane.[/spoiler]','2026-09-08 12:00:00+00'),
       (1000002,'demo',1000001,'Anonymous','',E'>>1000001\nA small paper lighthouse. Still working on the roof.','2026-09-08 12:05:00+00')
ON CONFLICT (id) DO NOTHING;
SELECT setval('content.post_number', GREATEST((SELECT last_value FROM content.post_number), (SELECT max(id) FROM content.posts)));
