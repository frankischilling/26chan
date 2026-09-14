-- Source posting cleanup/admission only; these do not advertise markup support.
ALTER TABLE content.boards
    ADD COLUMN comment_max_lines integer NOT NULL DEFAULT 70
        CHECK (comment_max_lines BETWEEN 0 AND 16000),
    ADD COLUMN comment_spoiler_cleanup boolean NOT NULL DEFAULT false;

-- Active CATEGORY=ws inheritance and board overrides in the supplied source.
-- Future boards require explicit operator configuration. Historical text stays.
UPDATE content.boards SET comment_max_lines=100 WHERE slug IN (
    '3','a','adv','an','asp','biz','c','cgl','ck','cm','co','diy','fa','fit','g','gd','his','int','jp','k','lgbt','lit','m','mlp','mu','n','news','o','out','p','po','pw','qa','qst','sci','sp','test','tg','toy','trv','tv','v','vg','vip','vm','vmg','vp','vr','vrpg','vst','vt','w','wsg','wsr','x','xs'
);
UPDATE content.boards SET comment_max_lines=50 WHERE slug IN ('b','bant');
UPDATE content.boards SET comment_spoiler_cleanup=true WHERE slug IN (
    'a','co','jp','lit','m','mlp','news','qst','r9k','s4s','test','tg','tv','u','v','vg','vip','vm','vmg','vp','vr','vrpg','vst','vt'
);
