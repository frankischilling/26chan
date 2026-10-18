INSERT INTO admission.rules(board,pattern) VALUES('upfile','synthetic predecessor filter');
INSERT INTO admission.network_rules(boards,range_start,range_end)
VALUES('upfile',3221225985,3221225985);
CREATE TABLE public.owned_file_policy_before AS
 SELECT jsonb_build_object(
  'rules',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.rules r),
  'network',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.network_rules r),
  'policy',(SELECT to_jsonb(p) FROM admission.policy p),
  'capacity',(SELECT to_jsonb(c) FROM admission.capacity c)) AS snapshot;
