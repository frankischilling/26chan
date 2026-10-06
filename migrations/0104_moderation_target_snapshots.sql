-- Saved-content evidence for newly recorded grouped thread options and image
-- spoiler actions. Historical rows and older binaries retain an all-NULL
-- snapshot. Do not backfill: today's post is not evidence of an earlier action.
ALTER TABLE content.moderation_audit
    ADD COLUMN snapshot_version smallint,
    ADD COLUMN snapshot_name text CHECK (octet_length(snapshot_name) <= 255),
    ADD COLUMN snapshot_trip text CHECK (
        octet_length(snapshot_trip) <= 13
        AND snapshot_trip ~ '^(![./0-9A-Za-z]{10}|!![+/0-9A-Za-z]{11})$'
    ),
    ADD COLUMN snapshot_capcode text CHECK (
        snapshot_capcode IN ('mod','admin','admin_highlight','manager','developer','founder')
    ),
    ADD COLUMN snapshot_subject text CHECK (octet_length(snapshot_subject) <= 1020),
    ADD COLUMN snapshot_comment text CHECK (octet_length(snapshot_comment) <= 2097152),
    ADD COLUMN snapshot_comment_format smallint CHECK (
        snapshot_comment_format = 0
        OR snapshot_comment_format BETWEEN 8 AND 15
        OR snapshot_comment_format BETWEEN 24 AND 31
        OR snapshot_comment_format BETWEEN 40 AND 47
        OR snapshot_comment_format BETWEEN 56 AND 63
        OR snapshot_comment_format BETWEEN 104 AND 111
        OR snapshot_comment_format BETWEEN 120 AND 127
    ),
    ADD COLUMN snapshot_staff_authorized_limits boolean,
    ADD COLUMN snapshot_wordfiltered boolean,
    ADD COLUMN snapshot_image_spoiler boolean,
    ADD COLUMN snapshot_filename text CHECK (octet_length(snapshot_filename) <= 255),
    ADD COLUMN snapshot_dice_result text CHECK (
        octet_length(snapshot_dice_result) >= 1 AND octet_length(snapshot_dice_result) <= 1024
        AND snapshot_dice_result !~ '[[:cntrl:]]'
    ),
    ADD COLUMN snapshot_fortune_text text CHECK (
        octet_length(snapshot_fortune_text) >= 1 AND octet_length(snapshot_fortune_text) <= 256
        AND snapshot_fortune_text !~ '[[:cntrl:]]'
    ),
    ADD COLUMN snapshot_fortune_color text CHECK (snapshot_fortune_color ~ '^#[0-9a-f]{6}$'),
    ADD CONSTRAINT moderation_audit_snapshot_randomizers CHECK (
        (snapshot_fortune_text IS NULL) = (snapshot_fortune_color IS NULL)
        AND NOT (snapshot_dice_result IS NOT NULL AND snapshot_fortune_text IS NOT NULL)
    ),
    ADD CONSTRAINT moderation_audit_snapshot_shape CHECK ((
        (snapshot_version IS NULL
         AND snapshot_name IS NULL AND snapshot_trip IS NULL AND snapshot_capcode IS NULL
         AND snapshot_subject IS NULL AND snapshot_comment IS NULL
         AND snapshot_comment_format IS NULL AND snapshot_staff_authorized_limits IS NULL
         AND snapshot_wordfiltered IS NULL AND snapshot_image_spoiler IS NULL
         AND snapshot_filename IS NULL AND snapshot_dice_result IS NULL
         AND snapshot_fortune_text IS NULL AND snapshot_fortune_color IS NULL)
        OR
        (snapshot_version = 1 AND action IN ('thread-options','spoiler','unspoiler')
         AND snapshot_name IS NOT NULL AND snapshot_subject IS NOT NULL
         AND snapshot_comment IS NOT NULL AND snapshot_comment_format IS NOT NULL
         AND snapshot_staff_authorized_limits IS NOT NULL
         AND snapshot_wordfiltered IS NOT NULL AND snapshot_image_spoiler IS NOT NULL)
    ) IS TRUE);

-- Bounds admit saved historical content, independently of today's posting
-- branches, board policy and formatter. Empty saved names are intentional.
-- Retain only a wordfiltered boolean: the payload can retain pre-normalization
-- URL context absent from the final saved comment; neither it nor search text
-- belongs in this evidence. No media bytes, keys or identity/network secrets.
-- Existing board_staff table-level INSERT/SELECT grants cover these columns.
-- No UPDATE/DELETE grant, helper, owner, target FK or mask-constraint change.
