-- Separate operator-owned identity from runtime authentication authority.
REVOKE ALL ON ALL TABLES IN SCHEMA staff_identity FROM board_auth;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA staff_identity FROM board_auth;
ALTER TABLE staff_identity.accounts ADD COLUMN username text UNIQUE;
ALTER TABLE staff_identity.accounts ADD COLUMN user_handle text UNIQUE;
ALTER TABLE staff_identity.accounts ADD CONSTRAINT account_name_shape CHECK (username IS NULL OR username ~ '^[a-zA-Z0-9_-]{1,64}$');

CREATE TABLE staff_identity.invitations (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash)=32),
    account_id bigint NOT NULL UNIQUE REFERENCES staff_identity.accounts(id),
    expires_at timestamptz NOT NULL
);
CREATE TABLE staff_identity.ceremonies (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash)=32),
    account_id bigint NOT NULL REFERENCES staff_identity.accounts(id),
    kind text NOT NULL CHECK (kind IN ('enroll','login')),
    state jsonb NOT NULL,
    invitation_hash bytea,
    expires_at timestamptz NOT NULL DEFAULT clock_timestamp() + interval '3 minutes'
);
CREATE INDEX ceremony_expiration ON staff_identity.ceremonies(expires_at);
CREATE TABLE staff_identity.sessions (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash)=32),
    csrf_hash bytea NOT NULL CHECK (octet_length(csrf_hash)=32),
    account_id bigint NOT NULL REFERENCES staff_identity.accounts(id),
    credential_id bytea NOT NULL REFERENCES staff_identity.credentials(id) ON DELETE CASCADE,
    authenticated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL DEFAULT clock_timestamp() + interval '8 hours'
);
CREATE INDEX session_expiration ON staff_identity.sessions(expires_at);
CREATE TABLE content.moderation_audit (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id bigint NOT NULL,
    board text NOT NULL REFERENCES content.boards(slug),
    target_id bigint NOT NULL,
    action text NOT NULL CHECK (action IN ('close','reopen','sticky','unsticky','remove-post','remove-thread','resolve','dismiss')),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
GRANT SELECT ON staff_identity.accounts, staff_identity.credentials, staff_identity.invitations TO board_auth;
GRANT SELECT, INSERT, DELETE ON staff_identity.ceremonies, staff_identity.sessions TO board_auth;
-- Enrollment is the only runtime credential insertion capability. The invitation
-- and account are locked, checked and consumed within the same transaction.
CREATE FUNCTION staff_identity.enroll(invite bytea, credential_id bytea, passkey jsonb) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE target bigint;
BEGIN
    SELECT a.id INTO target FROM staff_identity.accounts a
      JOIN staff_identity.invitations i ON i.account_id=a.id
      WHERE i.token_hash=invite AND i.expires_at>clock_timestamp() AND a.revoked_at IS NULL
      AND a.role IN ('moderator','admin') FOR UPDATE OF a, i;
    IF target IS NULL THEN RAISE EXCEPTION 'Enrollment unavailable'; END IF;
    INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES (credential_id,target,passkey);
    DELETE FROM staff_identity.invitations WHERE token_hash=invite;
    RETURN target;
END $$;
REVOKE ALL ON FUNCTION staff_identity.enroll(bytea,bytea,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.enroll(bytea,bytea,jsonb) TO board_auth;

-- The authenticator's immutable identity and key cannot be replaced at runtime.
CREATE FUNCTION staff_identity.update_counter(key_id bytea, previous jsonb, updated jsonb) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
    IF (previous #- '{cred,counter}' #- '{cred,backup_state}') IS DISTINCT FROM
       (updated #- '{cred,counter}' #- '{cred,backup_state}') OR
       (updated #>> '{cred,counter}')::bigint < (previous #>> '{cred,counter}')::bigint THEN
       RAISE EXCEPTION 'Invalid credential update';
    END IF;
    UPDATE staff_identity.credentials SET credential=updated WHERE id=key_id AND credential=previous;
    RETURN FOUND;
END $$;
REVOKE ALL ON FUNCTION staff_identity.update_counter(bytea,jsonb,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.update_counter(bytea,jsonb,jsonb) TO board_auth;
GRANT UPDATE(slug) ON content.boards TO board_staff;
GRANT SELECT, INSERT ON content.moderation_audit TO board_staff;
GRANT USAGE ON SEQUENCE content.moderation_audit_id_seq TO board_staff;
