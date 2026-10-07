-- Staff HTTP guards authenticate the session and enforce board access before
-- these capability-scoped calls. Unposted upload receipts remain bearer-owned;
-- cancellation revokes the handle without deleting a running job or its output.
-- Preserve the reviewed implementations, public grants and private 0110 bridge.
SET LOCAL ROLE board_attachment_owner;
GRANT EXECUTE ON FUNCTION content.check_attachment_upload(text,text),
    content.cancel_attachment_upload(text,text) TO board_staff;
RESET ROLE;
