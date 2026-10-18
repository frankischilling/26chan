CREATE TABLE public.owned_upload_history_before(bans jsonb,audit jsonb,capacity jsonb,indexdef text);
GRANT USAGE ON SCHEMA public TO board_restriction_owner;
GRANT SELECT,INSERT ON public.owned_upload_history_before TO board_restriction_owner;
SET ROLE board_restriction_owner;
SELECT restrictions.gate(true);
-- Synthetic historical snapshots: no live posts, template records or assets
-- are required to preserve already-recorded moderation events.
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,
 public_reason,private_reason,account_id,charged_bytes,template_id,upload_observation,normalized_upload_md5)
SELECT 'a',196,decode(repeat('19',32),'hex'),false,true,'2024-01-01+00','2024-01-01+00',
 'Owned history','Synthetic',1,640,226,'known',decode(repeat('62',16),'hex') FROM generate_series(1,3);
UPDATE restrictions.bans SET active=false,retired_at='2024-01-02+00',retired_by=1,retirement='unban'
 WHERE id=(SELECT min(id) FROM restrictions.bans);
UPDATE restrictions.bans SET warning=false,ends_at='2024-01-02+00'
 WHERE id=(SELECT max(id) FROM restrictions.bans);
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,
 public_reason,private_reason,account_id,charged_bytes,template_id,upload_observation)
SELECT 'a',196,decode(repeat('19',32),'hex'),false,true,'2024-01-01+00','2024-01-01+00',
 'Owned history','Synthetic',1,640,226,observation FROM unnest(ARRAY['unknown','absent']) observation;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,
 public_reason,private_reason,account_id,charged_bytes,template_id,upload_observation,normalized_upload_md5)
SELECT 'a',196,decode(repeat('19',32),'hex'),false,true,'2024-01-01+00','2024-01-01+00',
 'Owned history','Synthetic',1,640,6,'known',decode(repeat('63',16),'hex') FROM generate_series(1,3);
INSERT INTO restrictions.bans(board,actor_hash,global,warning,starts_at,ends_at,
 public_reason,private_reason,charged_bytes,origin,file_rule_id,file_digest)
SELECT 'a',decode(repeat('19',32),'hex'),true,false,'2024-01-01+00',NULL,
 'Owned file rule','Synthetic',640,'file',226,decode(repeat('64',16),'hex') FROM generate_series(1,3);
INSERT INTO restrictions.audit(ban_id,action,account_id)
 SELECT id,CASE WHEN warning THEN 'warning' ELSE 'ban' END,account_id FROM restrictions.bans;
UPDATE restrictions.capacity SET used_bytes=(SELECT sum(charged_bytes) FROM restrictions.bans) WHERE singleton;
INSERT INTO public.owned_upload_history_before SELECT
 (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b),
 (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a),
 (SELECT to_jsonb(c) FROM restrictions.capacity c),
 pg_get_indexdef('restrictions.restriction_upload_history'::regclass);
RESET ROLE;
