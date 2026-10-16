-- Run through an actual board_public connection, not SET ROLE from migration.
SELECT set_config('board.test_network_expected', :'expected_rows', false);
DO $$ BEGIN
 IF current_user<>'board_public' THEN RAISE EXCEPTION 'Expected public runtime connection'; END IF;
 IF (SELECT count(*) FROM content.network_admission_rules('demo',1000))<>current_setting('board.test_network_expected')::integer THEN
  RAISE EXCEPTION 'Reader scope or bounded overflow sentinel differs'; END IF;
 BEGIN
  PERFORM 1 FROM admission.network_rules;
  RAISE EXCEPTION 'Public role read private policy';
 EXCEPTION WHEN insufficient_privilege THEN NULL;
 END;
END $$;
