CREATE TABLE public.owned_notice_posts AS SELECT to_jsonb(p) AS value FROM content.posts p;
CREATE TABLE public.owned_notice_reports AS SELECT to_jsonb(r) AS value FROM content.reports r;
SET ROLE board_restriction_owner;
ALTER DEFAULT PRIVILEGES IN SCHEMA content GRANT EXECUTE ON FUNCTIONS TO board_auth;
ALTER DEFAULT PRIVILEGES IN SCHEMA restrictions GRANT EXECUTE ON FUNCTIONS TO board_staff;
RESET ROLE;
