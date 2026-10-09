-- Store the short name from the source [s4s] TITLE override.
-- Preserve the applied import checksum and operator-edited titles.
UPDATE content.boards SET title='Sh*t 4chan Says' WHERE slug='s4s' AND title='[s4s] - Sh*t 4chan Says';
