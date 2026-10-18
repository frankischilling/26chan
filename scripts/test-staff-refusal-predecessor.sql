INSERT INTO staff_identity.accounts(role,allow_boards,deny_boards) VALUES('janitor',ARRAY['a'],'{}');
INSERT INTO staff_identity.credentials(id,account_id,credential)
 SELECT decode(repeat('19',32),'hex'),max(id),'{}' FROM staff_identity.accounts;
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
 SELECT id,decode(repeat('20',32),'hex'),account_id,id FROM staff_identity.credentials WHERE id=decode(repeat('19',32),'hex');
CREATE TABLE public.owned_staff_refusal_before AS SELECT
 (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM staff_identity.accounts a) accounts,
 (SELECT jsonb_agg(to_jsonb(c) ORDER BY id) FROM staff_identity.credentials c) credentials,
 (SELECT jsonb_agg(to_jsonb(s) ORDER BY token_hash) FROM staff_identity.sessions s) sessions,
 (SELECT jsonb_agg(to_jsonb(i)) FROM post_secrets.staff_post_intents i) intents;
