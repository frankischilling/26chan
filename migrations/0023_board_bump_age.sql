ALTER TABLE content.boards
    ADD COLUMN permasage_hours integer NOT NULL DEFAULT 0
        CHECK (permasage_hours >= 0);
-- Operator-owned policy. Runtime column grants exclude policy updates.
