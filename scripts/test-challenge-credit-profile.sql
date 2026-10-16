\set ON_ERROR_STOP on
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at)
SELECT decode(repeat('71',32),'hex'),decode(repeat('72',32),'hex'),decode(repeat('73',32),'hex'),decode(repeat('74',32),'hex'),
 n,n,n,n,n+3600 FROM (SELECT extract(epoch FROM statement_timestamp())::bigint n) t;
SET LOCAL ROLE board_anonymous_owner;
DO $tests$
DECLARE n bigint:=extract(epoch FROM statement_timestamp())::bigint; test record; profile record;
 session_hash bytea:=decode(repeat('71',32),'hex'); network bytea:=decode(repeat('72',32),'hex');
 address bytea:=decode(repeat('73',32),'hex'); environment bytea:=decode(repeat('74',32),'hex');
 unverified_key bytea; canonical_key bytea; identity_id uuid:=gen_random_uuid();
BEGIN
 FOR test IN SELECT * FROM (VALUES
   (259199,5,0,0), (259200,5,1,3600),
   (1295999,20,1,3600), (1296000,20,2,7200),
   (7775999,100,2,7200), (7776000,100,3,10800)
  ) cases(age,posts,credits,seconds) LOOP
  UPDATE post_secrets.anonymous_sessions SET created_at=n-test.age,network_at=n-test.age,
   address_at=n-60,activity_at=n,posts=test.posts,pending=0 WHERE token_hash=session_hash;
  SELECT * INTO profile FROM verification_private.credit_profile(session_hash,network,address,environment,n);
  IF profile.credit_count<>test.credits OR profile.ttl_seconds<>test.seconds
     OR profile.can_spend IS DISTINCT FROM (test.credits>0) THEN
   RAISE EXCEPTION 'Credit tier boundary differs from source'; END IF;
 END LOOP;
 UPDATE post_secrets.anonymous_sessions SET created_at=n-259200,network_at=n-259200,
  address_at=n-59,activity_at=n,posts=4,pending=1 WHERE token_hash=session_hash;
 SELECT * INTO profile FROM verification_private.credit_profile(session_hash,network,address,environment,n);
 IF NOT profile.can_spend OR profile.credit_count<>0 THEN
  RAISE EXCEPTION 'Address-age grant gate incorrectly changed spending eligibility'; END IF;
 UPDATE post_secrets.anonymous_sessions SET address_at=n-60 WHERE token_hash=session_hash;
 SELECT * INTO profile FROM verification_private.credit_profile(session_hash,network,address,environment,n);
 IF profile.credit_count<>1 THEN RAISE EXCEPTION 'Pending pre-post activity was not counted'; END IF;
 SELECT * INTO profile FROM verification_private.credit_profile(session_hash,session_hash,address,environment,n);
 IF profile.can_spend OR profile.credit_count<>0 THEN
  RAISE EXCEPTION 'A changed network retained young-session credit eligibility'; END IF;
 UPDATE post_secrets.anonymous_sessions SET posts=9,pending=0 WHERE token_hash=session_hash;
 SELECT * INTO profile FROM verification_private.credit_profile(session_hash,session_hash,address,environment,n);
 IF NOT profile.can_spend OR profile.credit_count<>0 THEN
  RAISE EXCEPTION 'Known active browser lost spending or gained a fresh-address grant'; END IF;
 UPDATE post_secrets.anonymous_sessions SET change_score=10 WHERE token_hash=session_hash;
 SELECT * INTO profile FROM verification_private.credit_profile(session_hash,session_hash,address,environment,n);
 IF profile.can_spend THEN RAISE EXCEPTION 'Network churn bypassed the source known-user rule'; END IF;
 UPDATE post_secrets.anonymous_sessions SET created_at=n-7776000,network_at=n-7776000,
  activity_at=n-604800,posts=100,change_score=0 WHERE token_hash=session_hash;
 SELECT * INTO profile FROM verification_private.credit_profile(session_hash,network,address,environment,n);
 IF profile.can_spend OR profile.credit_count<>0 THEN RAISE EXCEPTION 'Idle activity earned credits'; END IF;
 UPDATE post_secrets.anonymous_sessions SET created_at=n,network_at=n,address_at=n,
  activity_at=n,verified_level=1,posts=0,pending=0 WHERE token_hash=session_hash;
 SELECT * INTO profile FROM verification_private.credit_profile(session_hash,network,address,environment,n);
 IF profile.can_spend OR profile.credit_count<>0 THEN RAISE EXCEPTION 'Verification alone minted credits'; END IF;
 unverified_key:=profile.identity_key;
 INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
  created_at,network_at,address_at,environment_at,expires_at)
 VALUES(decode(repeat('75',32),'hex'),network,address,environment,n,n,n,n,n+3600);
 SELECT * INTO profile FROM verification_private.credit_profile(decode(repeat('75',32),'hex'),network,address,environment,n);
 IF profile.identity_key=unverified_key THEN RAISE EXCEPTION 'Distinct anonymous browsers shared credits'; END IF;
 INSERT INTO verification_private.identities(id,email_hash) VALUES(identity_id,decode(repeat('76',32),'hex'));
 INSERT INTO verification_private.browser_identities(session_hash,identity_id)
 VALUES(session_hash,identity_id),(decode(repeat('75',32),'hex'),identity_id);
 SELECT * INTO profile FROM verification_private.credit_profile(session_hash,network,address,environment,n);
 canonical_key:=profile.identity_key;
 IF canonical_key=unverified_key THEN RAISE EXCEPTION 'Email credit identity reused the anonymous namespace'; END IF;
 SELECT * INTO profile FROM verification_private.credit_profile(decode(repeat('75',32),'hex'),network,address,environment,n);
 IF profile.identity_key<>canonical_key THEN RAISE EXCEPTION 'Verified browsers lost shared credit identity'; END IF;
END $tests$;
RESET ROLE;
ROLLBACK;
