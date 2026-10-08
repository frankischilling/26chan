-- Source flag types, finite choices and distinct persisted display labels.
CREATE FUNCTION content.board_flag_label(kind text,code text) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE SET search_path=pg_catalog,pg_temp AS $$ SELECT CASE kind WHEN 'pol' THEN CASE code WHEN 'AC' THEN 'Anarcho-Capitalist' WHEN 'AN' THEN 'Anarchist' WHEN 'BL' THEN 'Black Lives Matter' WHEN 'CF' THEN 'Confederate' WHEN 'CM' THEN 'Commie' WHEN 'CT' THEN 'Catalonia' WHEN 'DM' THEN 'Democrat' WHEN 'EU' THEN 'European' WHEN 'FC' THEN 'Fascist' WHEN 'GN' THEN 'Gadsden' WHEN 'GY' THEN 'LGBT' WHEN 'JH' THEN 'Jihadi' WHEN 'KN' THEN 'Kekistani' WHEN 'MF' THEN 'Muslim' WHEN 'NB' THEN 'National Bolshevik' WHEN 'NT' THEN 'NATO' WHEN 'NZ' THEN 'Nazi' WHEN 'PC' THEN 'Hippie' WHEN 'PR' THEN 'Pirate' WHEN 'RE' THEN 'Republican' WHEN 'TM' THEN 'DEUS VULT' WHEN 'MZ' THEN 'Task Force Z' WHEN 'TR' THEN 'Tree Hugger' WHEN 'UN' THEN 'United Nations' WHEN 'WP' THEN 'White Supremacist' END WHEN 'mlp' THEN CASE code WHEN '4CC' THEN '4cc /mlp/' WHEN 'ADA' THEN 'Adagio Dazzle' WHEN 'AN' THEN 'Anon' WHEN 'ANF' THEN 'Anonfilly' WHEN 'APB' THEN 'Apple Bloom' WHEN 'AJ' THEN 'Applejack' WHEN 'AB' THEN 'Aria Blaze' WHEN 'AU' THEN 'Autumn Blaze' WHEN 'BB' THEN 'Bon Bon' WHEN 'BM' THEN 'Big Mac' WHEN 'BP' THEN 'Berry Punch' WHEN 'BS' THEN 'Babs Seed' WHEN 'CL' THEN 'Changeling' WHEN 'CO' THEN 'Coco Pommel' WHEN 'CG' THEN 'Cozy Glow' WHEN 'CHE' THEN 'Cheerilee' WHEN 'CB' THEN 'Cherry Berry' WHEN 'DAY' THEN 'Daybreaker' WHEN 'DD' THEN 'Daring Do' WHEN 'DER' THEN 'Derpy Hooves' WHEN 'DT' THEN 'Diamond Tiara' WHEN 'DIS' THEN 'Discord' WHEN 'EQA' THEN 'EqG Applejack' WHEN 'EQF' THEN 'EqG Fluttershy' WHEN 'EQP' THEN 'EqG Pinkie Pie' WHEN 'EQR' THEN 'EqG Rainbow Dash' WHEN 'EQT' THEN 'EqG Trixie' WHEN 'EQI' THEN 'EqG Twilight Sparkle' WHEN 'EQS' THEN 'EqG Sunset Shimmer' WHEN 'ERA' THEN 'EqG Rarity' WHEN 'FAU' THEN 'Fausticorn' WHEN 'FLE' THEN 'Fleur de lis' WHEN 'FL' THEN 'Fluttershy' WHEN 'GI' THEN 'Gilda' WHEN 'HT' THEN 'Hitch Trailblazer' WHEN 'IZ' THEN 'Izzy Moonbow' WHEN 'LI' THEN 'Limestone' WHEN 'LT' THEN 'Lord Tirek' WHEN 'LY' THEN 'Lyra Heartstrings' WHEN 'MA' THEN 'Marble' WHEN 'MAU' THEN 'Maud' WHEN 'MIN' THEN 'Minuette' WHEN 'NI' THEN 'Nightmare Moon' WHEN 'NUR' THEN 'Nurse Redheart' WHEN 'OCT' THEN 'Octavia' WHEN 'PAR' THEN 'Parasprite' WHEN 'PC' THEN 'Princess Cadance' WHEN 'PCE' THEN 'Princess Celestia' WHEN 'PI' THEN 'Pinkie Pie' WHEN 'PLU' THEN 'Princess Luna' WHEN 'PM' THEN 'Pinkamena' WHEN 'PP' THEN 'Pipp Petals' WHEN 'QC' THEN 'Queen Chrysalis' WHEN 'RAR' THEN 'Rarity' WHEN 'RD' THEN 'Rainbow Dash' WHEN 'RLU' THEN 'Roseluck' WHEN 'S1L' THEN 'S1 Luna' WHEN 'SCO' THEN 'Scootaloo' WHEN 'SHI' THEN 'Shining Armor' WHEN 'SIL' THEN 'Silver Spoon' WHEN 'SON' THEN 'Sonata Dusk' WHEN 'SP' THEN 'Spike' WHEN 'SPI' THEN 'Spitfire' WHEN 'SS' THEN 'Sunny Starscout' WHEN 'STA' THEN 'Star Dancer' WHEN 'STL' THEN 'Starlight Glimmer' WHEN 'SPT' THEN 'Sprout' WHEN 'SUN' THEN 'Sunburst' WHEN 'SUS' THEN 'Sunset Shimmer' WHEN 'SWB' THEN 'Sweetie Belle' WHEN 'TFA' THEN 'TFH Arizona' WHEN 'TFO' THEN 'TFH Oleander' WHEN 'TFP' THEN 'TFH Paprika' WHEN 'TFS' THEN 'TFH Shanty' WHEN 'TFT' THEN 'TFH Tianhuo' WHEN 'TFV' THEN 'TFH Velvet' WHEN 'TP' THEN 'TFH Pom' WHEN 'TS' THEN 'Tempest Shadow' WHEN 'TWI' THEN 'Twilight Sparkle' WHEN 'TX' THEN 'Trixie' WHEN 'VS' THEN 'Vinyl Scratch' WHEN 'ZE' THEN 'Zecora' WHEN 'ZS' THEN 'Zipp Storm' END WHEN 'lgbt' THEN CASE code WHEN 'AAP' THEN 'AAP' WHEN 'ACE' THEN 'Asexual' WHEN 'ACH' THEN 'Achillean' WHEN 'AFB' THEN 'AFAB' WHEN 'AGP' THEN 'AGP' WHEN 'AGR' THEN 'Agender' WHEN 'ALL' THEN 'LGBT' WHEN 'ALY' THEN 'Ally' WHEN 'AMB' THEN 'AMAB' WHEN 'AND' THEN 'Androgynous' WHEN 'ARO' THEN 'Aromantic' WHEN 'BCH' THEN 'Butch' WHEN 'BI' THEN 'Bisexual' WHEN 'BOY' THEN 'Boymoder' WHEN 'BR' THEN 'Bear' WHEN 'CHR' THEN 'Chaser' WHEN 'CIS' THEN 'Cis' WHEN 'DOM' THEN 'Dom' WHEN 'DRO' THEN 'Demiromantic' WHEN 'DSX' THEN 'Demisexual' WHEN 'FBY' THEN 'Femboy' WHEN 'FFB' THEN 'FtM Femboy' WHEN 'FR' THEN 'FtM Repressor' WHEN 'GAY' THEN 'Gay' WHEN 'GFL' THEN 'Genderfluid' WHEN 'GQR' THEN 'Genderqueer' WHEN 'HFB' THEN 'HRT Femboy' WHEN 'HON' THEN 'Hon' WHEN 'HST' THEN 'HSTS' WHEN 'INT' THEN 'Intersex' WHEN 'LAB' THEN 'Labrys' WHEN 'LES' THEN 'Lesbian' WHEN 'MBT' THEN 'MtF Butch' WHEN 'MR' THEN 'MtF Repressor' WHEN 'NB' THEN 'Nonbinary' WHEN 'OG' THEN 'Original' WHEN 'PAN' THEN 'Pansexual' WHEN 'PBI' THEN 'Prison Bi' WHEN 'PG' THEN 'Prison Gay' WHEN 'PLY' THEN 'Poly' WHEN 'PNR' THEN 'Pooner' WHEN 'PRG' THEN 'Progress' WHEN 'QES' THEN 'Questioning' WHEN 'QR' THEN 'Queer' WHEN 'REP' THEN 'Repressor' WHEN 'SPH' THEN 'Sapphic' WHEN 'STR' THEN 'Straight' WHEN 'SUB' THEN 'Sub' WHEN 'SW' THEN 'Switch' WHEN 'TF' THEN 'Transfem' WHEN 'TKH' THEN 'Twinkhon' WHEN 'TMA' THEN 'Transmasc' WHEN 'TNK' THEN 'Twink' WHEN 'TRN' THEN 'Transgender' WHEN 'UKR' THEN 'Woke' END WHEN 'test' THEN CASE code WHEN 'FL1' THEN 'Flag 1' WHEN 'FL2' THEN 'Flag 2' END END $$;
CREATE FUNCTION content.board_flag_codes(kind text) RETURNS text[]
LANGUAGE sql IMMUTABLE PARALLEL SAFE SET search_path=pg_catalog,pg_temp AS $$ SELECT CASE kind WHEN 'pol' THEN ARRAY['AC','AN','BL','CF','CM','CT','DM','EU','FC','GN','GY','JH','KN','MF','NB','NT','NZ','PC','PR','RE','TM','MZ','TR','UN','WP']::text[] WHEN 'mlp' THEN ARRAY['4CC','ADA','AN','ANF','APB','AJ','AB','AU','BB','BM','BP','BS','CL','CO','CG','CHE','CB','DAY','DD','DER','DT','DIS','EQA','EQF','EQP','EQR','EQT','EQI','EQS','ERA','FAU','FLE','FL','GI','HT','IZ','LI','LT','LY','MA','MAU','MIN','NI','NUR','OCT','PAR','PC','PCE','PI','PLU','PM','PP','QC','RAR','RD','RLU','S1L','SCO','SHI','SIL','SON','SP','SPI','SS','STA','STL','SPT','SUN','SUS','SWB','TFA','TFO','TFP','TFS','TFT','TFV','TP','TS','TWI','TX','VS','ZE','ZS']::text[] WHEN 'lgbt' THEN ARRAY['AAP','ACE','ACH','AFB','AGP','AGR','ALL','ALY','AMB','AND','ARO','BCH','BI','BOY','BR','CHR','CIS','DOM','DRO','DSX','FBY','FFB','FR','GAY','GFL','GQR','HFB','HON','HST','INT','LAB','LES','MBT','MR','NB','OG','PAN','PBI','PG','PLY','PNR','PRG','QES','QR','REP','SPH','STR','SUB','SW','TF','TKH','TMA','TNK','TRN','UKR']::text[] WHEN 'test' THEN ARRAY['FL1','FL2']::text[] ELSE '{}'::text[] END $$;
REVOKE ALL ON FUNCTION content.board_flag_label(text,text),content.board_flag_codes(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.board_flag_label(text,text) TO board_public,board_staff,board_attachment_owner,board_staff_post_owner;
ALTER TABLE content.boards ADD COLUMN board_flag_type text NOT NULL DEFAULT 'pol'
  CHECK (board_flag_type IN ('pol','mlp','lgbt','test'));
ALTER TABLE content.boards DROP CONSTRAINT boards_board_flags_check;
ALTER TABLE content.boards ADD CONSTRAINT boards_board_flags_check CHECK (
  CASE WHEN cardinality(board_flags)=0 THEN true
  WHEN array_ndims(board_flags)=1 AND array_lower(board_flags,1)=1 THEN
    cardinality(board_flags)<=83 AND array_position(board_flags,NULL) IS NULL
    AND board_flags <@ content.board_flag_codes(board_flag_type)
  ELSE false END);
ALTER TABLE content.posts ADD COLUMN board_flag_type text NOT NULL DEFAULT 'pol'
  CHECK (board_flag_type IN ('pol','mlp','lgbt','test'));
ALTER TABLE content.posts DROP CONSTRAINT posts_board_flag_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_board_flag_check CHECK (
  board_flag IS NULL OR content.board_flag_label(board_flag_type,board_flag) IS NOT NULL);
GRANT SELECT(board_flag_type) ON content.boards TO board_attachment_owner,board_staff_post_owner;
CREATE OR REPLACE FUNCTION content.apply_post_flag() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_geo boolean; v_flags text[]; v_selected text; v_type text;
BEGIN
  NEW.board_flag_type := 'pol';
  IF NEW.capcode IS NOT NULL THEN NEW.country:=NULL; NEW.country_name:=NULL; NEW.board_flag:=NULL; NEW.flag_name:=NULL; RETURN NEW; END IF;
  SELECT country_flags,board_flags,board_flag_type INTO v_geo,v_flags,v_type FROM content.boards WHERE slug=NEW.board FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
  v_selected := coalesce(nullif(current_setting('board.flag',true),''),'0');
  NEW.country := NULL; NEW.country_name := NULL; NEW.board_flag := NULL; NEW.flag_name := NULL;
  IF v_selected <> '0' THEN
    IF NOT v_selected=ANY(v_flags) OR content.board_flag_label(v_type,v_selected) IS NULL THEN
      RAISE EXCEPTION 'Invalid board flag.' USING ERRCODE='23514';
    END IF;
    NEW.board_flag := v_selected; NEW.board_flag_type := v_type;
    NEW.flag_name := content.board_flag_label(v_type,v_selected);
  ELSIF v_geo THEN
    NEW.country := nullif(current_setting('board.country',true),'');
    NEW.country_name := nullif(current_setting('board.country_name',true),'');
    IF NEW.country IS NULL OR NEW.country_name IS NULL THEN
      RAISE EXCEPTION 'Country flags are unavailable.' USING ERRCODE='23514';
    END IF;
  END IF;
  RETURN NEW;
END $$;
UPDATE content.boards b SET board_flag_type=policy.kind,board_flags=policy.codes
FROM (VALUES
('a','pol',ARRAY[]::text[]),
('aco','pol',ARRAY[]::text[]),
('c','pol',ARRAY[]::text[]),
('d','pol',ARRAY[]::text[]),
('e','pol',ARRAY[]::text[]),
('f','pol',ARRAY[]::text[]),
('g','pol',ARRAY[]::text[]),
('gif','pol',ARRAY[]::text[]),
('h','pol',ARRAY[]::text[]),
('his','pol',ARRAY[]::text[]),
('hr','pol',ARRAY[]::text[]),
('k','pol',ARRAY[]::text[]),
('m','pol',ARRAY[]::text[]),
('n','pol',ARRAY[]::text[]),
('o','pol',ARRAY[]::text[]),
('p','pol',ARRAY[]::text[]),
('r','pol',ARRAY[]::text[]),
('s','pol',ARRAY[]::text[]),
('t','pol',ARRAY[]::text[]),
('u','pol',ARRAY[]::text[]),
('v','pol',ARRAY[]::text[]),
('w','pol',ARRAY[]::text[]),
('wg','pol',ARRAY[]::text[]),
('i','pol',ARRAY[]::text[]),
('ic','pol',ARRAY[]::text[]),
('cm','pol',ARRAY[]::text[]),
('y','pol',ARRAY[]::text[]),
('an','pol',ARRAY[]::text[]),
('cgl','pol',ARRAY[]::text[]),
('ck','pol',ARRAY[]::text[]),
('co','pol',ARRAY[]::text[]),
('fa','pol',ARRAY[]::text[]),
('fit','pol',ARRAY[]::text[]),
('jp','pol',ARRAY[]::text[]),
('mlp','mlp',ARRAY['4CC','ADA','AN','ANF','APB','AJ','AB','AU','BB','BM','BP','BS','CL','CO','CG','CHE','CB','DAY','DD','DER','DT','DIS','EQA','EQF','EQP','EQR','EQT','EQI','EQS','ERA','FAU','FLE','FL','GI','HT','IZ','LI','LT','LY','MA','MAU','MIN','NI','NUR','OCT','PAR','PC','PCE','PI','PLU','PM','PP','QC','RAR','RD','RLU','S1L','SCO','SHI','SIL','SON','SP','SPI','SS','STA','STL','SPT','SUN','SUS','SWB','TFA','TFO','TFP','TFS','TFT','TFV','TP','TS','TWI','TX','VS','ZE','ZS']::text[]),
('mu','pol',ARRAY[]::text[]),
('po','pol',ARRAY[]::text[]),
('sp','pol',ARRAY[]::text[]),
('tg','pol',ARRAY[]::text[]),
('toy','pol',ARRAY[]::text[]),
('trv','pol',ARRAY[]::text[]),
('tv','pol',ARRAY[]::text[]),
('x','pol',ARRAY[]::text[]),
('b','pol',ARRAY[]::text[]),
('soc','pol',ARRAY[]::text[]),
('r9k','pol',ARRAY[]::text[]),
('test','test',ARRAY[]::text[]),
('adv','pol',ARRAY[]::text[]),
('lit','pol',ARRAY[]::text[]),
('int','pol',ARRAY[]::text[]),
('sci','pol',ARRAY[]::text[]),
('3','pol',ARRAY[]::text[]),
('vp','pol',ARRAY[]::text[]),
('diy','pol',ARRAY[]::text[]),
('pol','pol',ARRAY['AC','AN','BL','CF','CM','CT','DM','EU','FC','GN','GY','JH','KN','MF','NB','NT','NZ','PC','PR','RE','MZ','TM','TR','UN','WP']::text[]),
('hc','pol',ARRAY[]::text[]),
('vg','pol',ARRAY[]::text[]),
('hm','pol',ARRAY[]::text[]),
('j','pol',ARRAY[]::text[]),
('wsg','pol',ARRAY[]::text[]),
('out','pol',ARRAY[]::text[]),
('lgbt','lgbt',ARRAY[]::text[]),
('vr','pol',ARRAY[]::text[]),
('gd','pol',ARRAY[]::text[]),
('s4s','pol',ARRAY[]::text[]),
('biz','pol',ARRAY[]::text[]),
('qa','pol',ARRAY[]::text[]),
('trash','pol',ARRAY[]::text[]),
('news','pol',ARRAY[]::text[]),
('wsr','pol',ARRAY[]::text[]),
('qst','pol',ARRAY[]::text[]),
('bant','pol',ARRAY[]::text[]),
('vip','pol',ARRAY[]::text[]),
('vrpg','pol',ARRAY[]::text[]),
('vmg','pol',ARRAY[]::text[]),
('vst','pol',ARRAY[]::text[]),
('vt','pol',ARRAY[]::text[]),
('vm','pol',ARRAY[]::text[]),
('pw','pol',ARRAY[]::text[]),
('xs','pol',ARRAY[]::text[]),
('asp','pol',ARRAY[]::text[]),
('qb','pol',ARRAY[]::text[])) policy(slug,kind,codes) WHERE b.slug=policy.slug;
