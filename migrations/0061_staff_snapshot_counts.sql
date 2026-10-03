-- Staff board snapshots use the same bounded aggregate as public snapshots.
-- This exposes only the existing count, never poster fingerprints or addresses.
SET LOCAL ROLE board_poster_count_owner;
GRANT EXECUTE ON FUNCTION content.unique_posters(text,bigint) TO board_staff;
RESET ROLE;
