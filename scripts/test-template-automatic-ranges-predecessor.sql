CREATE TABLE public.owned_template_ranges_before(templates jsonb,events jsonb);
GRANT SELECT,INSERT ON public.owned_template_ranges_before TO board_restriction_owner;
GRANT USAGE ON SCHEMA public TO board_restriction_owner;
SET ROLE board_restriction_owner;
INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason)
 VALUES(1900000101,'Owned global preset','moderator',true,false,60,'Owned public','Owned private'),
       (1900000102,'Owned local preset','moderator',false,true,0,'Owned warning','Owned private');
INSERT INTO public.owned_template_ranges_before
 SELECT (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM restrictions.templates t) templates,
        (SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM restrictions.automatic_ranges e) events;
RESET ROLE;
