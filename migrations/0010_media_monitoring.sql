CREATE SCHEMA monitoring;
REVOKE ALL ON SCHEMA monitoring FROM PUBLIC;

CREATE INDEX media_monitor_active ON media.jobs (state, expires_at, created_at)
    WHERE state IN ('receiving', 'queued', 'processing');
CREATE INDEX media_monitor_recent_failures ON media.jobs (updated_at, failure)
    WHERE state = 'failed';

-- The migration owner supplies the underlying reads. The observer receives no
-- base-schema/table rights. Aggregation also makes the view non-updatable.
CREATE VIEW monitoring.media_queue WITH (security_barrier = true) AS
SELECT policy.capacity::bigint AS capacity,
       active.receiving, active.queued, active.processing,
       active.expired_receiving, active.expired_queued, active.expired_processing,
       active.oldest_queued_seconds,
       failures.intake_failed, failures.abandoned, failures.processing_failed,
       failures.invalid_output, failures.retry_exhausted
FROM media.queue_policy AS policy
CROSS JOIN (
    SELECT count(*) FILTER (WHERE state = 'receiving') AS receiving,
           count(*) FILTER (WHERE state = 'queued') AS queued,
           count(*) FILTER (WHERE state = 'processing') AS processing,
           count(*) FILTER (WHERE state = 'receiving' AND expires_at <= statement_timestamp()) AS expired_receiving,
           count(*) FILTER (WHERE state = 'queued' AND expires_at <= statement_timestamp()) AS expired_queued,
           count(*) FILTER (WHERE state = 'processing' AND expires_at <= statement_timestamp()) AS expired_processing,
           COALESCE(GREATEST(0, floor(extract(epoch FROM
               statement_timestamp() - min(created_at) FILTER (WHERE state = 'queued')))), 0)::bigint AS oldest_queued_seconds
    FROM media.jobs
    WHERE state IN ('receiving', 'queued', 'processing')
) AS active
CROSS JOIN (
    SELECT count(*) FILTER (WHERE failure = 'intake_failed') AS intake_failed,
           count(*) FILTER (WHERE failure = 'abandoned') AS abandoned,
           count(*) FILTER (WHERE failure = 'processing_failed') AS processing_failed,
           count(*) FILTER (WHERE failure = 'invalid_output') AS invalid_output,
           count(*) FILTER (WHERE failure = 'retry_exhausted') AS retry_exhausted
    FROM media.jobs
    WHERE state = 'failed'
      AND updated_at >= statement_timestamp() - interval '15 minutes'
      AND updated_at <= statement_timestamp()
) AS failures
WHERE policy.singleton;

REVOKE ALL ON monitoring.media_queue FROM PUBLIC;
GRANT USAGE ON SCHEMA monitoring TO board_monitor;
GRANT SELECT ON monitoring.media_queue TO board_monitor;
