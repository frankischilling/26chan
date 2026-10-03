-- Undead already participates in source image limits, tail size and eviction.
-- The staff runtime changes this one thread field through scoped moderation.
GRANT UPDATE(undead) ON content.threads TO board_staff;

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK(action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler',
        'undead','unundead'));
