-- Jobs awaiting an unavailable worker must also have a finite retention deadline.
ALTER TABLE media.jobs DROP CONSTRAINT jobs_check;
UPDATE media.jobs SET expires_at = clock_timestamp() + interval '1 hour' WHERE state = 'queued';
ALTER TABLE media.jobs ADD CONSTRAINT jobs_expiration_state
    CHECK ((state IN ('receiving', 'queued', 'processing')) = (expires_at IS NOT NULL));
