-- Attachment insertion runs as this restricted NOLOGIN function owner.
-- Its ID trigger needs both source policy columns; posting runtimes gain no grants.
GRANT SELECT(meta_board,poster_id_no_heaven) ON content.boards TO board_attachment_owner;
