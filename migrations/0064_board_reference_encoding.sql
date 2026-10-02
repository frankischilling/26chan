-- Correct source board titles decoded with the historical Windows default.
-- Preserve the applied migration checksum and operator-edited titles.
UPDATE content.boards SET title='Pokémon' WHERE slug='vp' AND title='PokÃ©mon';
