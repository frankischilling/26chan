-- Owned restored-evidence fixtures qualify review issuance and refusal. These
-- synthetic private digests do not claim to exercise public media admission.
DO $$ DECLARE c record; req bigint; outcome jsonb; rules_before bigint; bans_before bigint;
 revision_before bigint; queue jsonb; result_mode smallint; result_days integer; matches integer; tested integer:=0;
BEGIN
 FOR c IN SELECT * FROM (VALUES
  ('image',false,'known',1,NULL,0,0,false),('rejectimage',false,'known',0,NULL,0,0,false),
  ('image',true,'known',NULL,NULL,0,0,false),('rejectimage',true,'unknown',NULL,NULL,0,0,false),
  ('none',false,'unknown',NULL,NULL,0,0,false),('image',false,'absent',NULL,NULL,0,0,false),
  ('rejectimage',false,'unknown',NULL,'P0207',0,0,false),
  ('image',false,'known',NULL,'P134C',128,0,false),
  ('image',false,'known',NULL,'P134C',0,4096,false),
  ('image',false,'known',1,NULL,0,0,true)
 ) AS cases(policy,warning,observation,wanted,error_code,fill,reason_bytes,zero_after) LOOP
  BEGIN
   INSERT INTO admission.file_rules(board,digest,mode)
    SELECT '',decode(md5('owned-quota-'||i),'hex'),0 FROM generate_series(1,c.fill) i;
   SELECT count(*) INTO rules_before FROM admission.file_rules;
   SELECT count(*) INTO bans_before FROM restrictions.bans;
   SELECT revision INTO revision_before FROM admission.policy;
   SET LOCAL ROLE board_restriction_owner;
   INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason,
    request_save_policy,request_blacklist_policy,request_can_warn)
   VALUES(9101,'Owned image review','janitor',false,false,86400,CASE WHEN c.reason_bytes=0 THEN 'Owned image reason' ELSE repeat('x',c.reason_bytes) END,'','none',c.policy,true);
   SET LOCAL ROLE board_staff;
   req:=content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9101,c.warning,false);
   RESET ROLE;
   UPDATE restrictions.requests SET upload_observation=c.observation,
    normalized_upload_md5=CASE WHEN c.observation='known' THEN decode(repeat('ab',16),'hex') END WHERE id=req;
   UPDATE restrictions.templates SET warning=true,seconds=0 WHERE id=9101 AND c.zero_after;
   SET LOCAL ROLE board_auth;
   queue:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,'rqown',0);
   RESET ROLE;
   IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(queue->'requests') r WHERE (r->>'id')::bigint=req
    AND (r->>'can_accept')::boolean=(c.warning OR c.policy='none' OR c.observation IN('known','absent'))) THEN
    RAISE EXCEPTION 'Queue did not explain missing private file evidence'; END IF;
   SET LOCAL ROLE board_staff;
   BEGIN
    outcome:=content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,req,true,NULL,NULL,NULL,NULL,NULL);
    IF c.error_code IS NOT NULL THEN RAISE EXCEPTION 'Unavailable file effect accepted'; END IF;
   EXCEPTION WHEN SQLSTATE 'P0207' OR SQLSTATE 'P134C' THEN
    IF SQLSTATE IS DISTINCT FROM c.error_code THEN RAISE; END IF;
   END;
   RESET ROLE;
   IF c.error_code IS NOT NULL THEN
    IF NOT EXISTS(SELECT 1 FROM restrictions.requests WHERE id=req)
     OR (SELECT count(*) FROM restrictions.bans)<>bans_before
     OR (SELECT count(*) FROM admission.file_rules)<>rules_before
     OR (SELECT revision FROM admission.policy)<>revision_before
     OR EXISTS(SELECT 1 FROM restrictions.request_reviews WHERE request_id=req) THEN
     RAISE EXCEPTION 'File refusal left partial review effects'; END IF;
   ELSE
    IF outcome->>'changed'<>'true' OR outcome->>'cleared'<>'1'
     OR EXISTS(SELECT 1 FROM restrictions.requests WHERE id=req) THEN
     RAISE EXCEPTION 'File review did not commit one outcome'; END IF;
    IF NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE id=(outcome->>'ban_id')::bigint
     AND warning=(c.warning OR c.zero_after)) THEN
     RAISE EXCEPTION 'Stored duration collapsed the distinct caller warning branch'; END IF;
    SET LOCAL ROLE board_staff;
    BEGIN
     PERFORM content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,req,true,NULL,NULL,NULL,NULL,NULL);
     RAISE EXCEPTION 'Repeated image-rule approval succeeded';
    EXCEPTION WHEN SQLSTATE 'P134N' THEN NULL; END;
    RESET ROLE;
    IF c.wanted IS NULL THEN
     IF (SELECT count(*) FROM admission.file_rules)<>rules_before
      OR (SELECT revision FROM admission.policy)<>revision_before
      OR EXISTS(SELECT 1 FROM restrictions.request_file_rules WHERE request_id=req) THEN
      RAISE EXCEPTION 'Warning, none or absent evidence wrote an image rule'; END IF;
    ELSE
     IF (SELECT count(*) FROM admission.file_rules)<>rules_before+1
      OR (SELECT revision FROM admission.policy)<>revision_before+1
      OR NOT EXISTS(SELECT 1 FROM restrictions.request_file_rules f JOIN restrictions.bans b ON b.id=f.ban_id
       WHERE f.request_id=req AND f.reviewer_id=b.account_id AND f.accepted_template=9101
        AND f.requested_by=(SELECT id FROM staff_identity.accounts WHERE username='owned-request-janitor')) THEN
      RAISE EXCEPTION 'Image rule authority, generation or attribution differs'; END IF;
     SET LOCAL ROLE board_public;
     SELECT count(*) INTO matches FROM content.file_admission_rules('rqother')
      WHERE digest=decode(repeat('ab',16),'hex') AND board='';
     SELECT mode,ban_days INTO result_mode,result_days FROM content.file_admission_rules('rqother')
      WHERE digest=decode(repeat('ab',16),'hex') AND board='';
     RESET ROLE;
     IF matches<>1 OR result_mode<>c.wanted OR result_days<>(CASE WHEN c.wanted=1 THEN -1 ELSE 0 END) THEN
      RAISE EXCEPTION 'Issued global rule did not reach actual file admission'; END IF;
    END IF;
   END IF;
   RAISE EXCEPTION 'Rollback owned file case' USING ERRCODE='ZJ001';
  EXCEPTION WHEN SQLSTATE 'ZJ001' THEN NULL;
  END;
  tested:=tested+1;
 END LOOP;
 RAISE NOTICE 'Request image-rule writer passes % owned source outcomes',tested;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF has_table_privilege('board_staff','admission.file_rules','INSERT')
  OR has_column_privilege('board_staff','admission.policy','revision','UPDATE')
  OR has_sequence_privilege('board_staff','admission.file_rules_id_seq','USAGE')
  OR has_function_privilege('board_staff','restrictions.issue_request_file_rule(restrictions.requests,restrictions.templates,bigint,bigint,boolean)','EXECUTE')
  OR has_table_privilege('board_auth','restrictions.request_file_rules','SELECT') THEN
  RAISE EXCEPTION 'Runtime role gained direct file-rule authority'; END IF;
END $$;
BEGIN;
GRANT USAGE ON SEQUENCE admission.file_rules_id_seq TO board_public;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'File-rule sequence grant drift was ignored'; END IF; END $$;
ROLLBACK;
BEGIN;
GRANT INSERT ON admission.file_rules TO board_staff;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'File-rule writer grant drift was ignored'; END IF; END $$;
ROLLBACK;
-- Leave one real pending request with owned restored digest evidence for the
-- separate-connection policy-lock and populated-restore checks.
BEGIN;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason,
 request_save_policy,request_blacklist_policy)
VALUES(9101,'Owned persistent image review','janitor',false,false,86400,'Owned image reason','','none','image');
SET LOCAL ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9101,false,false);
RESET ROLE;
UPDATE restrictions.requests SET upload_observation='known',normalized_upload_md5=decode(repeat('ab',16),'hex')
 WHERE board='rqown' AND post_id=207004;
COMMIT;
