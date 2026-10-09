-- Source REPLIES_SHOWN; synthetic/operator boards inherit the source default.
-- Configuration is independent of thread posting and response resource limits.
ALTER TABLE content.boards
    ADD COLUMN replies_shown integer NOT NULL DEFAULT 5
    CONSTRAINT boards_replies_shown CHECK (replies_shown BETWEEN 0 AND 5);

-- Only audited source overrides; preserve every other board setting.
UPDATE content.boards b SET replies_shown=policy.replies_shown
FROM (VALUES ('b',3),('bant',3),('t',1),('vg',0)) AS policy(slug,replies_shown)
WHERE b.slug=policy.slug;
