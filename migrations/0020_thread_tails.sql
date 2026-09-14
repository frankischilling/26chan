ALTER TABLE content.boards ADD COLUMN json_tail_size integer NOT NULL DEFAULT 0
    CHECK (json_tail_size BETWEEN 0 AND 500);
ALTER TABLE content.threads ADD COLUMN undead boolean NOT NULL DEFAULT false;
-- Policy is operator-owned. Existing public/staff column grants do not permit
-- changing either field. The old sticky+undead flag doubles the configured tail.
