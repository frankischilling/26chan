CREATE TABLE public.owned_file_restrictions_before(bans jsonb,audit jsonb,capacity jsonb);
GRANT SELECT,INSERT ON public.owned_file_restrictions_before TO board_restriction_owner;
GRANT USAGE ON SCHEMA public TO board_restriction_owner;
SET ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,
 public_reason,private_reason,account_id,charged_bytes)
VALUES('a',188,decode(repeat('88',32),'hex'),false,true,'2024-01-01+00','2024-01-01+00',
 'Synthetic predecessor warning','Private predecessor',1,1024);
INSERT INTO restrictions.audit(ban_id,action,account_id)
 SELECT id,'warning',1 FROM restrictions.bans WHERE post_id=188 AND actor_hash=decode(repeat('88',32),'hex');
UPDATE restrictions.capacity SET used_bytes=used_bytes+1024 WHERE singleton;
INSERT INTO public.owned_file_restrictions_before SELECT
 (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b) AS bans,
 (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a) AS audit,
 (SELECT to_jsonb(c) FROM restrictions.capacity c) AS capacity;
RESET ROLE;
