INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('qcredit193','Owned credit upgrade','Synthetic',2000,100,100,100,10);
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at,posts)
SELECT decode(repeat('a1',32),'hex'),decode(repeat('a2',32),'hex'),decode(repeat('a3',32),'hex'),decode(repeat('a4',32),'hex'),
 n,n,n,n,n+3600,5 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) t;
INSERT INTO verification_private.receipts(token_hash,peer_hash,session_hash,purpose,board,target,context_hash,
 from_credit,suspicious,unsolved_before,may_grant_credit,created_at,expires_at)
VALUES(decode(repeat('b1',32),'hex'),decode(repeat('b2',32),'hex'),decode(repeat('a1',32),'hex'),
 'thread','qcredit193',0,decode(repeat('b3',32),'hex'),false,false,1,true,clock_timestamp(),clock_timestamp()+interval '30 seconds');
INSERT INTO verification_private.challenges(token_hash,answer_hash,suspicious,peer_hash,session_hash,
 purpose,board,target,context_hash,created_at,expires_at)
VALUES(decode(repeat('c1',32),'hex'),decode(repeat('c2',32),'hex'),false,decode(repeat('c3',32),'hex'),decode(repeat('a1',32),'hex'),
 'thread','qcredit193',0,decode(repeat('c4',32),'hex'),clock_timestamp(),clock_timestamp()+interval '60 seconds');
INSERT INTO verification_private.credits(identity_key,remaining,expires_at)
 VALUES(decode(repeat('d1',32),'hex'),2,clock_timestamp()+interval '1 hour');
CREATE FUNCTION public.owned_credit_request_rows() RETURNS jsonb LANGUAGE sql AS $$
 SELECT jsonb_build_array(
  (SELECT jsonb_agg(to_jsonb(r) ORDER BY token_hash) FROM verification_private.receipts r),
  (SELECT jsonb_agg(to_jsonb(r) ORDER BY token_hash) FROM verification_private.challenges r),
  (SELECT jsonb_agg(to_jsonb(r) ORDER BY identity_key) FROM verification_private.credits r),
  (SELECT jsonb_agg(to_jsonb(r) ORDER BY token_hash) FROM post_secrets.anonymous_sessions r),
  (SELECT jsonb_agg(to_jsonb(r) ORDER BY post_id) FROM verification_private.post_handoffs r),
  (SELECT to_jsonb(r) FROM verification_private.policy r))
$$;
REVOKE ALL ON FUNCTION public.owned_credit_request_rows() FROM PUBLIC;
CREATE TABLE public.owned_credit_request_before AS
 SELECT public.owned_credit_request_rows() AS rows,
 (SELECT prosrc FROM pg_proc WHERE oid='content.check_public_post_challenge()'::regprocedure) AS source;
REVOKE ALL ON public.owned_credit_request_before FROM PUBLIC;
