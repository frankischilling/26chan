-- Owned populated-upgrade checks only. Do not replace any historical baseline.
DO $$ BEGIN
 IF to_regclass('post_secrets.report_post_snapshots') IS NULL THEN RETURN; END IF;
 IF content.report_snapshots_ready() IS DISTINCT FROM true
 OR EXISTS(TABLE post_secrets.report_post_snapshots)
 OR (SELECT count(*) FROM post_secrets.report_snapshot_capacity)<>1
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_snapshot_capacity
     WHERE singleton AND byte_limit=67108864 AND used_bytes=0)
 OR (SELECT count(*) FROM post_secrets.report_snapshot_catalog)<>1
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_snapshot_catalog
     WHERE singleton AND definition_hash=post_secrets.report_snapshot_catalog_hash())
 THEN RAISE EXCEPTION 'Capture upgrade changed legacy reports or failed private readiness'; END IF;
 IF to_regclass('public.upgraded_rows') IS NOT NULL THEN
  INSERT INTO public.upgraded_rows VALUES(
    'post_secrets.report_snapshot_capacity',
    jsonb_build_object('singleton',true,'byte_limit',67108864,'used_bytes',0));
  INSERT INTO public.upgraded_rows VALUES(
    'post_secrets.report_snapshot_catalog',
    jsonb_build_object('singleton',true,'definition_hash',
      '\x'||encode(post_secrets.report_snapshot_catalog_hash(),'hex')));
 END IF;
END $$;
