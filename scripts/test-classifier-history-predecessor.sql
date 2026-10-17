CREATE TABLE public.owned_classifier_before AS
 SELECT sha256(convert_to('owned-classifier-upgrade','UTF8')) AS token, '{}'::jsonb AS state;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at,posts,pending,verified_level)
 SELECT token,token,token,token,n-100,n-100,n-100,n-100,n+3600,12,1,0
 FROM public.owned_classifier_before CROSS JOIN (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) t;
UPDATE public.owned_classifier_before b SET state=to_jsonb(s)
 FROM post_secrets.anonymous_sessions s WHERE s.token_hash=b.token;
GRANT USAGE ON SCHEMA public TO board_anonymous_owner;
GRANT SELECT ON public.owned_classifier_before TO board_anonymous_owner;
-- Exercise role-specific default privileges; the new migration must remove
-- these grants from its new objects before attesting their private shape.
ALTER DEFAULT PRIVILEGES FOR ROLE board_migrator IN SCHEMA post_secrets GRANT SELECT ON TABLES TO board_public;
SET ROLE board_anonymous_owner;
ALTER DEFAULT PRIVILEGES IN SCHEMA post_secrets GRANT EXECUTE ON FUNCTIONS TO board_public;
RESET ROLE;
