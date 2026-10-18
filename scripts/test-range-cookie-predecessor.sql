INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,activity_at,action_at,expires_at,
 verified_level,posts,images,threads,reports,pending,change_score)
VALUES(decode(repeat('61',32),'hex'),decode(repeat('62',32),'hex'),decode(repeat('63',32),'hex'),decode(repeat('64',32),'hex'),
 100000,100001,100002,100003,100004,100005,4102444800,1,2,3,4,5,6,7);
CREATE TABLE public.owned_range_cookie_before AS SELECT jsonb_build_object(
 'sessions',(SELECT jsonb_agg(to_jsonb(s) ORDER BY token_hash) FROM post_secrets.anonymous_sessions s),
 'policy',(SELECT to_jsonb(p) FROM post_secrets.anonymous_policy p),
 'table_acl',(SELECT relacl::text FROM pg_class WHERE oid='post_secrets.anonymous_sessions'::regclass)) AS snapshot;
