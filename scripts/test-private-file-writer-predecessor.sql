CREATE TABLE public.owned_private_writer_before(bans jsonb,audit jsonb,capacity jsonb);
GRANT USAGE ON SCHEMA public TO board_restriction_owner;
GRANT SELECT,INSERT ON public.owned_private_writer_before TO board_restriction_owner;
SET ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,
 public_reason,private_reason,account_id,charged_bytes)
VALUES('a',189,decode(repeat('89',32),'hex'),false,true,'2024-01-01+00','2024-01-01+00',
 'Synthetic staff warning','Private predecessor',1,1024);
INSERT INTO restrictions.bans(board,actor_hash,global,warning,starts_at,ends_at,
 public_reason,private_reason,charged_bytes,origin,file_rule_id,file_digest)
VALUES('a',decode(repeat('90',32),'hex'),true,true,'2024-01-01 12:00:00+00','2024-01-01+00',
 'Synthetic automatic warning','Private predecessor',1024,'file',189,decode(repeat('42',16),'hex'));
INSERT INTO restrictions.audit(ban_id,action,account_id)
 SELECT id,'warning',account_id FROM restrictions.bans WHERE actor_hash IN(decode(repeat('89',32),'hex'),decode(repeat('90',32),'hex'));
UPDATE restrictions.capacity SET used_bytes=used_bytes+2048 WHERE singleton;
INSERT INTO public.owned_private_writer_before SELECT
 (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b),
 (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a),
 (SELECT to_jsonb(c) FROM restrictions.capacity c);
RESET ROLE;
