\ir test-classifier-history-predecessor.sql
SET ROLE board_anonymous_owner;
SELECT post_secrets.record_classifier_history(token,'block_bm_bot') FROM public.owned_classifier_before;
ALTER DEFAULT PRIVILEGES IN SCHEMA content GRANT EXECUTE ON FUNCTIONS TO board_staff;
RESET ROLE;
ALTER TABLE public.owned_classifier_before ADD COLUMN history jsonb, ADD COLUMN catalog_hash bytea;
UPDATE public.owned_classifier_before b SET
 history=(SELECT jsonb_agg(to_jsonb(h) ORDER BY kind) FROM post_secrets.classifier_history h WHERE h.token_hash=b.token),
 catalog_hash=(SELECT definition_hash FROM post_secrets.classifier_history_catalog WHERE singleton);
