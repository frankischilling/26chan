INSERT INTO admission.file_rules(id,board,digest,mode,ban_days,public_reason,description)
 VALUES(191,'a',decode(repeat('11',16),'hex'),1,3,'Owned public reason','Owned private description');
INSERT INTO admission.automatic_ban_exemptions VALUES('192.0.2.0/24',true);
INSERT INTO admission.dmca_warnings VALUES(decode(repeat('19',32),'hex'),'2026-01-01 00:00:00+00');
CREATE TABLE public.owned_capability_file_before AS SELECT
 (SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.file_rules r) rules,
 (SELECT jsonb_agg(to_jsonb(e) ORDER BY network) FROM admission.automatic_ban_exemptions e) exemptions,
 (SELECT jsonb_agg(to_jsonb(w) ORDER BY actor_hash) FROM admission.dmca_warnings w) warnings,
 (SELECT to_jsonb(p) FROM admission.policy p) policy,
 (SELECT to_jsonb(c) FROM admission.file_effect_capacity c) capacity;

CREATE TABLE public.owned_capability_restrictions_before(bans jsonb,audit jsonb,capacity jsonb);
GRANT SELECT,INSERT ON public.owned_capability_restrictions_before TO board_restriction_owner;
GRANT USAGE ON SCHEMA public TO board_restriction_owner;
SET ROLE board_restriction_owner;
SELECT restrictions.record_file_ban(decode(repeat('19',32),'hex'),NULL,'a',191,
 decode(repeat('11',16),'hex'),3,false,'Owned public reason','Owned private reason','UTC');
INSERT INTO public.owned_capability_restrictions_before SELECT
 (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b),
 (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a),
 (SELECT to_jsonb(c) FROM restrictions.capacity c);
RESET ROLE;
