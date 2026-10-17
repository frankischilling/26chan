\ir test-country-classifier-predecessor.sql
ALTER DEFAULT PRIVILEGES FOR ROLE board_migrator IN SCHEMA post_secrets GRANT SELECT,INSERT ON TABLES TO board_staff;
ALTER DEFAULT PRIVILEGES FOR ROLE board_migrator IN SCHEMA post_secrets GRANT USAGE,SELECT ON SEQUENCES TO board_staff;
