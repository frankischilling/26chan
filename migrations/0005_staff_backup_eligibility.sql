-- webauthn-rs permits a passkey to become backup eligible after registration.
-- Only that one-way transition, counter advances and backup state may change.
CREATE OR REPLACE FUNCTION staff_identity.update_counter(key_id bytea, previous jsonb, updated jsonb) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
    IF jsonb_typeof(updated #> '{cred,counter}') IS DISTINCT FROM 'number' OR
       jsonb_typeof(updated #> '{cred,backup_eligible}') IS DISTINCT FROM 'boolean' OR
       jsonb_typeof(updated #> '{cred,backup_state}') IS DISTINCT FROM 'boolean' OR
       (previous #- '{cred,counter}' #- '{cred,backup_state}' #- '{cred,backup_eligible}') IS DISTINCT FROM
       (updated #- '{cred,counter}' #- '{cred,backup_state}' #- '{cred,backup_eligible}') OR
       (updated #>> '{cred,counter}')::bigint NOT BETWEEN 0 AND 4294967295 OR
       (updated #>> '{cred,counter}')::bigint < (previous #>> '{cred,counter}')::bigint OR
       ((previous #>> '{cred,backup_eligible}')::boolean AND NOT (updated #>> '{cred,backup_eligible}')::boolean) THEN
       RAISE EXCEPTION 'Invalid credential update';
    END IF;
    UPDATE staff_identity.credentials SET credential=updated WHERE id=key_id AND credential=previous;
    RETURN FOUND;
END $$;
REVOKE ALL ON FUNCTION staff_identity.update_counter(bytea,jsonb,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION staff_identity.update_counter(bytea,jsonb,jsonb) TO board_auth;
