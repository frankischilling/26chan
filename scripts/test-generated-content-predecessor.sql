-- Preserve ordinary and generated historical rows, including audit snapshots.
UPDATE content.posts SET comment_format=CASE id WHEN 1000001 THEN 232 ELSE 255 END
 WHERE board='demo' AND id IN(1000001,1000002);
UPDATE content.boards SET dice_roll=true,fortune_trip=true WHERE slug='demo';
UPDATE content.posts SET dice_result='Rolled 1 (1d1)' WHERE board='demo' AND id=1000001;
UPDATE content.posts SET fortune_text='Bad Luck',fortune_color='#7fec11' WHERE board='demo' AND id=1000002;
INSERT INTO content.moderation_audit(account_id,board,target_id,action,
 snapshot_version,snapshot_name,snapshot_subject,snapshot_comment,snapshot_comment_format,
 snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler)
SELECT 4242,board,id,'spoiler',1,name,subject,comment,comment_format,false,false,false
 FROM content.posts WHERE board='demo' AND id IN(1000001,1000002) ORDER BY id;
CREATE TABLE public.owned_boards_before AS SELECT * FROM content.boards;
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
CREATE TABLE public.owned_audits_before AS SELECT * FROM content.moderation_audit;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.posts WHERE comment_format BETWEEN 232 AND 255) THEN
  RAISE EXCEPTION 'Populated predecessor fixture is missing'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE snapshot_comment_format BETWEEN 232 AND 255) THEN
  RAISE EXCEPTION 'Historical audit snapshot fixture is missing'; END IF;
END $$;
