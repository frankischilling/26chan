ALTER TABLE content.boards ADD COLUMN country_flags boolean NOT NULL DEFAULT false;
ALTER TABLE content.boards ADD COLUMN board_flags text[] NOT NULL DEFAULT '{}'
CHECK (cardinality(board_flags)<=25 AND board_flags <@ ARRAY['AC','AN','BL','CF','CM','CT','DM','EU','FC','GN','GY','JH','KN','MF','NB','NT','NZ','PC','PR','RE','MZ','TM','TR','UN','WP']::text[]
    AND array_position(board_flags,NULL) IS NULL);
ALTER TABLE content.posts ADD COLUMN country text CHECK (country IS NULL OR country=ANY(ARRAY['AD','AE','AF','AG','AI','AL','AM','AN','AO','AQ','AR','AS','AT','AU','AW','AX','AZ','BA','BB','BD','BE','BF','BG','BH','BI','BJ','BL','BM','BN','BO','BQ','BR','BS','BT','BV','BW','BY','BZ','CA','CC','CD','CF','CG','CH','CI','CK','CL','CM','CN','CO','CR','CS','CU','CV','CW','CX','CY','CZ','DE','DJ','DK','DM','DO','DZ','EC','EE','EG','EH','XE','ER','ES','ET','EU','FI','FJ','FK','FM','FO','FR','GA','GB','GD','GE','GF','GG','GH','GI','GL','GM','GN','GP','GQ','GR','GS','GT','GU','GW','GY','HK','HM','HN','HR','HT','HU','ID','IE','IL','IM','IN','IO','IQ','IR','IS','IT','JE','JM','JO','JP','KE','KG','KH','KI','KM','KN','KP','KR','KW','KY','KZ','LA','LB','LC','LI','LK','LR','LS','LT','LU','LV','LY','MA','MC','MD','ME','MF','MG','MH','MK','ML','MM','MN','MO','MP','MQ','MR','MS','MT','MU','MV','MW','MX','MY','MZ','NA','NC','NE','NF','NG','NI','NL','NO','NP','NR','NU','NZ','OM','PA','PE','PF','PG','PH','PK','PL','PM','PN','PR','PS','PT','PW','PY','QA','RE','RO','RS','RU','RW','SA','SB','SC','XS','SD','SE','SG','SH','SI','SJ','SK','SL','SM','SN','SO','SR','SS','ST','SV','SX','SY','SZ','TC','TD','TF','TG','TH','TJ','TK','TL','TM','TN','TO','TR','TT','TV','TW','TZ','UA','UG','UM','US','UY','UZ','VA','VC','VE','VG','VI','VN','VU','XW','WF','WS','XK','XX','YE','YT','ZA','ZM','ZW']::text[]));
ALTER TABLE content.posts ADD COLUMN country_name text CHECK (country_name IS NULL OR (octet_length(country_name) BETWEEN 1 AND 100 AND country_name !~ '[[:cntrl:]]'));
ALTER TABLE content.posts ADD COLUMN board_flag text CHECK (board_flag IS NULL OR board_flag=ANY(ARRAY['AC','AN','BL','CF','CM','CT','DM','EU','FC','GN','GY','JH','KN','MF','NB','NT','NZ','PC','PR','RE','MZ','TM','TR','UN','WP']::text[]));
ALTER TABLE content.posts ADD COLUMN flag_name text CHECK (flag_name IS NULL OR octet_length(flag_name) BETWEEN 1 AND 100);
ALTER TABLE content.posts ADD CONSTRAINT paired_post_flags CHECK (
    (country IS NULL)=(country_name IS NULL) AND (board_flag IS NULL)=(flag_name IS NULL)
    AND (country IS NULL OR board_flag IS NULL));

CREATE FUNCTION content.apply_post_flag() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_geo boolean; v_flags text[]; v_selected text;
BEGIN
    SELECT country_flags,board_flags INTO v_geo,v_flags FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    v_selected := coalesce(nullif(current_setting('board.flag',true),''),'0');
    NEW.country := NULL; NEW.country_name := NULL; NEW.board_flag := NULL; NEW.flag_name := NULL;
    IF v_selected <> '0' THEN
        IF NOT v_selected=ANY(v_flags) THEN
            RAISE EXCEPTION 'Invalid board flag.' USING ERRCODE='23514';
        END IF;
        NEW.board_flag := v_selected;
        NEW.flag_name := (ARRAY['Anarcho-Capitalist','Anarchist','Black Nationalist','Confederate','Communist','Catalonia','Democrat','European','Fascist','Gadsden','Gay','Jihadi','Kekistani','Muslim','National Bolshevik','NATO','Nazi','Hippie','Pirate','Republican','Task Force Z','Templar','Tree Hugger','United Nations','White Supremacist']::text[])[array_position(ARRAY['AC','AN','BL','CF','CM','CT','DM','EU','FC','GN','GY','JH','KN','MF','NB','NT','NZ','PC','PR','RE','MZ','TM','TR','UN','WP']::text[],v_selected)];
    ELSIF v_geo THEN
        NEW.country := nullif(current_setting('board.country',true),'');
        NEW.country_name := nullif(current_setting('board.country_name',true),'');
        IF NEW.country IS NULL OR NEW.country_name IS NULL THEN
            RAISE EXCEPTION 'Country flags are unavailable.' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.apply_post_flag() FROM PUBLIC;
CREATE TRIGGER apply_post_flag BEFORE INSERT ON content.posts FOR EACH ROW EXECUTE FUNCTION content.apply_post_flag();
GRANT SELECT(country_flags,board_flags) ON content.boards TO board_attachment_owner;
