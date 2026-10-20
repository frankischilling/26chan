-- Owned synthetic identities. The fixed encoded hash exercises SQL shape only;
-- password verification is qualified separately through the private HTTP API.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,captcha_enabled,max_comment_chars,
    reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('vipfixture','Owned VIP','Synthetic',false,2000,100,100,100,10),
      ('vipgated','Owned gated VIP','Synthetic',true,2000,100,100,100,10);
SELECT content.vip_manage(decode(repeat('11',32),'hex'),'Owned Verified Name',
    '$argon2id$v=19$m=19456,t=2,p=1$' || repeat('a',22) || '$' || repeat('b',43),true,true);
SELECT content.vip_manage(decode(repeat('12',32),'hex'),'Inactive Owned Name',
    '$argon2id$v=19$m=19456,t=2,p=1$' || repeat('a',22) || '$' || repeat('b',43),false,true);
RESET ROLE;
DO $$ BEGIN
    IF NOT content.public_vip_ready() OR NOT content.public_pass_ready() THEN
        RAISE EXCEPTION 'Owned VIP setup changed authority';
    END IF;
    IF EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_verifier',
        'board_mailer','board_media','board_media_read','board_media_intake','board_checkout']) r(name)
        WHERE has_schema_privilege(r.name,'verification_private','USAGE')
            OR has_table_privilege(r.name,'verification_private.vip_accounts','SELECT,INSERT,UPDATE,DELETE')
            OR has_table_privilege(r.name,'verification_private.vip_attempts','SELECT,INSERT,UPDATE,DELETE')
            OR has_table_privilege(r.name,'verification_private.vip_receipts','SELECT,INSERT,UPDATE,DELETE')) THEN
        RAISE EXCEPTION 'VIP raw table privilege escaped';
    END IF;
END $$;

SET LOCAL ROLE board_public;
DO $$ BEGIN
    BEGIN
        PERFORM content.vip_manage(decode(repeat('11',32),'hex'),NULL,NULL,false,false);
        RAISE EXCEPTION 'Public VIP management accepted';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN
        PERFORM content.vip_begin_login(decode(repeat('11',32),'hex'),decode(repeat('21',32),'hex'),
            decode(repeat('31',32),'hex'),decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),
            'vipfixture',0,NULL,NULL);
        RAISE EXCEPTION 'Public VIP issuer accepted';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
RESET ROLE;
SET LOCAL ROLE board_verifier;
DO $$ DECLARE result record; index integer;
BEGIN
    -- A bad password is counted; three reserved/failed attempts fill the hour.
    FOR index IN 1..3 LOOP
        SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),
            decode(repeat('21',32),'hex'),sha256(convert_to('failure-'||index,'UTF8')),
            decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),'vipfixture',0,NULL,NULL);
        IF result.status<>'verify' OR result.password_hash IS NULL THEN RAISE EXCEPTION 'Known lookup differs'; END IF;
        IF content.vip_finish_login(sha256(convert_to('failure-'||index,'UTF8')),
            decode(repeat('21',32),'hex'),false,sha256(convert_to('failed-receipt-'||index,'UTF8')))<>'rejected' THEN
            RAISE EXCEPTION 'Wrong credential accepted';
        END IF;
    END LOOP;
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),
        decode(repeat('21',32),'hex'),decode(repeat('31',32),'hex'),
        decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),'vipfixture',0,NULL,NULL);
    IF result.status<>'flood' OR result.password_hash IS NOT NULL THEN RAISE EXCEPTION 'VIP failure limit differs'; END IF;
    -- CAPTCHA enablement requires an actual current Pass receipt.
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),
        decode(repeat('22',32),'hex'),decode(repeat('32',32),'hex'),
        decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),'vipgated',0,NULL,NULL);
    IF result.status<>'ineligible' THEN RAISE EXCEPTION 'Ordinary CAPTCHA gate bypassed'; END IF;
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),
        decode(repeat('22',32),'hex'),decode(repeat('32',32),'hex'),
        decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),'vipgated',0,
        decode(repeat('61',32),'hex'),decode(repeat('71',32),'hex'));
    IF result.status<>'ineligible' THEN RAISE EXCEPTION 'Forged Pass receipt accepted'; END IF;
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('12',32),'hex'),
        decode(repeat('22',32),'hex'),decode(repeat('32',32),'hex'),
        decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),'vipfixture',0,NULL,NULL);
    IF result.status<>'verify' OR result.password_hash IS NOT NULL THEN RAISE EXCEPTION 'Inactive lookup leaked credential'; END IF;
    IF content.vip_finish_login(decode(repeat('32',32),'hex'),decode(repeat('22',32),'hex'),
        true,decode(repeat('62',32),'hex'))<>'rejected' THEN RAISE EXCEPTION 'Inactive VIP accepted'; END IF;
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),
        decode(repeat('23',32),'hex'),decode(repeat('33',32),'hex'),
        decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),'vipfixture',42,NULL,NULL);
    IF result.status<>'verify' THEN RAISE EXCEPTION 'Active lookup unavailable'; END IF;
    IF content.vip_finish_login(decode(repeat('33',32),'hex'),decode(repeat('24',32),'hex'),
        true,decode(repeat('63',32),'hex'))<>'rejected' THEN RAISE EXCEPTION 'Wrong peer completed'; END IF;
    IF content.vip_finish_login(decode(repeat('33',32),'hex'),decode(repeat('23',32),'hex'),
        true,decode(repeat('63',32),'hex'))<>'ready' THEN RAISE EXCEPTION 'Active VIP not issued'; END IF;
    IF content.vip_finish_login(decode(repeat('33',32),'hex'),decode(repeat('23',32),'hex'),
        true,decode(repeat('64',32),'hex'))<>'rejected' THEN RAISE EXCEPTION 'VIP completion replay accepted'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN
    IF (SELECT count(*) FROM verification_private.vip_receipts)<>1
        OR EXISTS(SELECT 1 FROM verification_private.vip_attempts WHERE peer=decode(repeat('23',32),'hex'))
        OR NOT EXISTS(SELECT 1 FROM verification_private.vip_accounts
            WHERE identifier_hash=decode(repeat('11',32),'hex') AND last_peer=decode(repeat('23',32),'hex')
                AND last_used IS NOT NULL AND credential_version=1)
        OR NOT EXISTS(SELECT 1 FROM verification_private.vip_receipts
            WHERE token_hash=decode(repeat('63',32),'hex') AND board='vipfixture' AND target=42
                AND peer=decode(repeat('23',32),'hex') AND anonymous_session=decode(repeat('41',32),'hex')
                AND context_hash=decode(repeat('51',32),'hex')
                AND expires_at-created_at=interval '30 seconds') THEN
        RAISE EXCEPTION 'VIP receipt, attempts or last-use effects differ';
    END IF;
END $$;
SET LOCAL ROLE board_verifier;
SELECT * FROM content.vip_begin_login(decode(repeat('11',32),'hex'),decode(repeat('25',32),'hex'),
    decode(repeat('35',32),'hex'),decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),
    'vipfixture',0,NULL,NULL);
RESET ROLE;
SET LOCAL ROLE board_migrator;
SELECT content.vip_manage(decode(repeat('11',32),'hex'),'Renamed Owned VIP',NULL,NULL,false);
RESET ROLE;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
    IF content.vip_finish_login(decode(repeat('35',32),'hex'),decode(repeat('25',32),'hex'),
        true,decode(repeat('65',32),'hex'))<>'rejected' THEN RAISE EXCEPTION 'Stale name version issued'; END IF;
END $$;
SELECT * FROM content.vip_begin_login(decode(repeat('11',32),'hex'),decode(repeat('26',32),'hex'),
    decode(repeat('36',32),'hex'),decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),
    'vipfixture',0,NULL,NULL);
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_attempts SET created_at=clock_timestamp()-interval '31 seconds'
    WHERE ticket=decode(repeat('36',32),'hex');
RESET ROLE;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
    IF content.vip_finish_login(decode(repeat('36',32),'hex'),decode(repeat('26',32),'hex'),
        true,decode(repeat('66',32),'hex'))<>'rejected' THEN RAISE EXCEPTION 'Expired attempt issued'; END IF;
END $$;
RESET ROLE;
SAVEPOINT catalog_drift;
SET LOCAL ROLE board_anonymous_owner;
GRANT SELECT ON verification_private.vip_accounts TO board_public;
RESET ROLE;
DO $$ BEGIN
    IF content.public_vip_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'VIP privilege drift unobserved'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
    BEGIN
        PERFORM content.vip_begin_login(decode(repeat('11',32),'hex'),decode(repeat('27',32),'hex'),
            decode(repeat('37',32),'hex'),decode(repeat('41',32),'hex'),decode(repeat('51',32),'hex'),
            'vipfixture',0,NULL,NULL);
        RAISE EXCEPTION 'VIP drift did not stop issuer';
    EXCEPTION WHEN SQLSTATE '55000' THEN NULL; END;
END $$;
ROLLBACK TO SAVEPOINT catalog_drift;
DO $$ BEGIN IF NOT content.public_vip_ready() THEN RAISE EXCEPTION 'VIP rollback did not restore guard'; END IF; END $$;
SAVEPOINT pass_backed;
SET LOCAL ROLE board_migrator;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
    created_at,network_at,address_at,environment_at,expires_at)
SELECT sha256('vip-anonymous'),sha256('vip-network'),sha256('vip-address'),sha256('vip-environment'),
    stamp,stamp,stamp,stamp,stamp+3600
FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint stamp) clock;
SET LOCAL ROLE board_anonymous_owner;
WITH account AS (
    INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
    VALUES(sha256('vip-owned-pass'),repeat('owned-placeholder',3),0,'2020-01-01',statement_timestamp()+interval '1 year')
    RETURNING id
)
INSERT INTO verification_private.pass_sessions
SELECT sha256('vip-pass-session'),id,1,statement_timestamp(),statement_timestamp()+interval '1 day' FROM account;
SET LOCAL ROLE board_verifier;
DO $$ DECLARE result record;
BEGIN
    IF (SELECT status FROM content.pass_write_receipt(sha256('vip-pass-session'),sha256('vip-pass-receipt'),
        sha256('vip-peer'),sha256('vip-login-scope'),sha256('vip-posting-scope'),sha256('vip-anonymous'),
        sha256('vip-pass-form'),'thread','vipgated',0))<>'ready' THEN RAISE EXCEPTION 'Owned Pass receipt unavailable'; END IF;
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),sha256('vip-peer'),
        sha256('vip-gated-ticket'),sha256('vip-anonymous'),sha256('vip-new-context'),'vipgated',0,
        sha256('vip-pass-receipt'),sha256('wrong-pass-form'));
    IF result.status<>'ineligible' THEN RAISE EXCEPTION 'Wrong Pass context admitted VIP'; END IF;
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),sha256('wrong-peer'),
        sha256('vip-gated-ticket'),sha256('vip-anonymous'),sha256('vip-new-context'),'vipgated',0,
        sha256('vip-pass-receipt'),sha256('vip-pass-form'));
    IF result.status<>'ineligible' THEN RAISE EXCEPTION 'Wrong Pass peer admitted VIP'; END IF;
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),sha256('vip-peer'),
        sha256('vip-gated-ticket'),sha256('vip-anonymous'),sha256('vip-new-context'),'vipgated',42,
        sha256('vip-pass-receipt'),sha256('vip-pass-form'));
    IF result.status<>'ineligible' THEN RAISE EXCEPTION 'Wrong Pass target admitted VIP'; END IF;
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),sha256('vip-peer'),
        sha256('vip-gated-ticket'),sha256('vip-anonymous'),sha256('vip-new-context'),'vipgated',0,
        sha256('vip-pass-receipt'),sha256('vip-pass-form'));
    IF result.status<>'verify' THEN RAISE EXCEPTION 'Current Pass failed VIP admission'; END IF;
    IF content.vip_finish_login(sha256('vip-gated-ticket'),sha256('vip-peer'),true,sha256('vip-gated-receipt'))<>'ready' THEN
        RAISE EXCEPTION 'Current Pass failed VIP receipt issuance';
    END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
    IF NOT EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('vip-pass-receipt'))
        OR NOT EXISTS(SELECT 1 FROM verification_private.vip_receipts WHERE token_hash=sha256('vip-gated-receipt')) THEN
        RAISE EXCEPTION 'VIP issuance consumed its independent Pass receipt';
    END IF;
END $$;
UPDATE verification_private.pass_entitlements SET status=1 WHERE token_hash=sha256('vip-owned-pass');
SET LOCAL ROLE board_verifier;
DO $$ DECLARE result record; BEGIN
    SELECT * INTO result FROM content.vip_begin_login(decode(repeat('11',32),'hex'),sha256('vip-peer'),
        sha256('vip-revoked-ticket'),sha256('vip-anonymous'),sha256('vip-new-context'),'vipgated',0,
        sha256('vip-pass-receipt'),sha256('vip-pass-form'));
    IF result.status<>'ineligible' THEN RAISE EXCEPTION 'Revoked Pass admitted VIP'; END IF;
END $$;
ROLLBACK TO SAVEPOINT pass_backed;
COMMIT;
