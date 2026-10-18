INSERT INTO admission.rules(board,pattern) VALUES('upban','Synthetic predecessor');
INSERT INTO admission.bans(peer,board,reason,expires_at)
 VALUES('192.0.2.185','upban','content','2000-01-01 00:00:00+00');
INSERT INTO admission.file_rules(board,digest) VALUES('upban',decode(repeat('42',16),'hex'));
CREATE TABLE public.owned_ban_exemptions_before AS
 SELECT jsonb_build_object(
  'rules',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.rules r),
  'files',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.file_rules r),
  'bans',(SELECT jsonb_agg(to_jsonb(b) ORDER BY peer) FROM admission.bans b),
  'hits',(SELECT jsonb_agg(to_jsonb(h) ORDER BY rule_id,peer) FROM admission.hits h),
  'policy',(SELECT to_jsonb(p) FROM admission.policy p),
  'capacity',(SELECT to_jsonb(c) FROM admission.capacity c)) AS snapshot;
