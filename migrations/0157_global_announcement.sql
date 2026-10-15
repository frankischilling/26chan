-- A single operator-published announcement uses the existing bounded local-page
-- editor. Preserve every page, news item and revision; runtime grants stay read-only.
DO $migration$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid='site_private.pages'::regclass AND conname='pages_check'
        AND convalidated AND NOT condeferrable
        AND pg_get_constraintdef(oid,true)=$expected$CHECK (board IS NULL AND (key = ANY (ARRAY['about'::text, 'faq'::text, 'rules'::text, 'contact'::text, 'feedback'::text, 'legal'::text, 'press'::text, 'advertise'::text, 'support'::text, 'japanese'::text, 'blog'::text])) OR board IS NOT NULL AND key = ('rules/'::text || board))$expected$
    ) THEN
        RAISE EXCEPTION 'Unexpected local page key constraint';
    END IF;
END
$migration$;
ALTER TABLE site_private.pages DROP CONSTRAINT pages_check;
ALTER TABLE site_private.pages ADD CONSTRAINT pages_check CHECK (
    (board IS NULL AND key IN ('about','faq','rules','contact','feedback','legal','press','advertise','support','japanese','blog','announcement'))
    OR (board IS NOT NULL AND key='rules/'||board)
);
