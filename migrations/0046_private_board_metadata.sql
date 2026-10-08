-- The constrained poster-count function reads posts and threads under its own
-- role. Their visibility policy needs only the board key, not board settings.
GRANT SELECT(slug) ON content.boards TO board_poster_count_owner;

ALTER TABLE post_secrets.op_peers ENABLE ROW LEVEL SECURITY;
CREATE POLICY op_peer_visibility ON post_secrets.op_peers USING (
    EXISTS(SELECT 1 FROM content.threads t WHERE t.id=op_peers.thread_id)
);
ALTER TABLE post_secrets.op_replies ENABLE ROW LEVEL SECURITY;
CREATE POLICY op_reply_visibility ON post_secrets.op_replies USING (
    EXISTS(SELECT 1 FROM content.posts p WHERE p.id=op_replies.post_id)
);
