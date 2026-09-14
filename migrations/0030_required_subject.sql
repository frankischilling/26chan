-- Active REQUIRE_SUBJECT defaults in the supplied qst/vg configuration.
-- Operator policy for new posts only; historical subjects and clocks stay intact.
ALTER TABLE content.boards
    ADD COLUMN require_subject boolean NOT NULL DEFAULT false;
UPDATE content.boards SET require_subject=true WHERE slug IN ('qst','vg');
