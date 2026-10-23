-- Optional, default-closed event accounts. All points and stocks are fictional.
-- Bootstrap the NOLOGIN board_event_owner before applying this migration.
-- Versions 0249/0250 belong to the independently reviewed upload-board work.
DO $$
DECLARE r record;
BEGIN
 SELECT * INTO r FROM pg_roles WHERE rolname='board_event_owner';
 IF NOT FOUND OR r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole
   OR r.rolreplication OR r.rolbypassrls OR r.rolconfig IS NOT NULL THEN
  RAISE EXCEPTION 'Event function owner is missing or unsafe';
 END IF;
 IF (SELECT count(*) FROM pg_auth_members WHERE roleid=r.oid)<>1 OR NOT EXISTS(
  SELECT 1 FROM pg_auth_members m JOIN pg_roles member ON member.oid=m.member
  WHERE m.roleid=r.oid AND member.rolname='board_migrator' AND m.set_option
    AND NOT m.inherit_option AND NOT m.admin_option)
  OR EXISTS(SELECT 1 FROM pg_auth_members WHERE member=r.oid)
  OR EXISTS(SELECT 1 FROM pg_class WHERE relowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_proc WHERE proowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_namespace WHERE nspowner=r.oid)
  OR EXISTS(SELECT 1 FROM pg_class c,LATERAL aclexplode(c.relacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_attribute c,LATERAL aclexplode(c.attacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_proc c,LATERAL aclexplode(c.proacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_namespace c,LATERAL aclexplode(c.nspacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_database c,LATERAL aclexplode(c.datacl) a WHERE a.grantee=r.oid)
  OR EXISTS(SELECT 1 FROM pg_default_acl c WHERE c.defaclrole=r.oid)
  OR EXISTS(SELECT 1 FROM pg_default_acl c,LATERAL aclexplode(c.defaclacl) a WHERE a.grantee=r.oid)
 THEN RAISE EXCEPTION 'Event owner has unexpected existing authority'; END IF;
 IF to_regnamespace('event_private') IS NOT NULL THEN
  RAISE EXCEPTION 'Event schema already exists';
 END IF;
END $$;

-- Granting one new NOLOGIN owner USAGE on content changes the namespace
-- fingerprint used by classifier/report admission. Verify and lock those
-- authorities before the grant; do not bless pre-existing privilege drift.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before optional event authority'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

CREATE SCHEMA event_private AUTHORIZATION board_migrator;
REVOKE ALL ON SCHEMA event_private FROM PUBLIC;
CREATE TABLE event_private.runs (
 id bigint PRIMARY KEY CHECK(id>0),
 kind smallint NOT NULL CHECK(kind IN(1,2)),
 enabled boolean NOT NULL DEFAULT false,
 starts_at bigint NOT NULL CHECK(starts_at BETWEEN 0 AND 253402300799),
 ends_at bigint NOT NULL CHECK(ends_at>=1 AND ends_at<=253402300799 AND ends_at>starts_at),
 account_capacity integer NOT NULL DEFAULT 10000 CHECK(account_capacity BETWEEN 1 AND 100000),
 account_count integer NOT NULL DEFAULT 0 CHECK(account_count BETWEEN 0 AND account_capacity),
 UNIQUE(id,kind)
);
CREATE UNIQUE INDEX one_enabled_event_per_kind ON event_private.runs(kind) WHERE enabled;
CREATE TABLE event_private.quotes (
 run_id bigint NOT NULL REFERENCES event_private.runs(id) ON DELETE CASCADE,
 stock smallint NOT NULL CHECK(stock BETWEEN 0 AND 17),
 price bigint NOT NULL CHECK(price>0),
 PRIMARY KEY(run_id,stock)
);
CREATE FUNCTION event_private.valid_owned(p_owned smallint[]) RETURNS boolean
LANGUAGE sql IMMUTABLE STRICT SET search_path=pg_catalog,pg_temp AS $$
 SELECT cardinality(p_owned)<=166 AND array_position(p_owned,NULL) IS NULL
 AND (cardinality(p_owned)=0 OR (array_ndims(p_owned)=1 AND array_lower(p_owned,1)=1))
 AND 0<=ALL(p_owned) AND 165>=ALL(p_owned)
 AND (SELECT count(DISTINCT v) FROM unnest(p_owned) v)=cardinality(p_owned)
$$;
REVOKE ALL ON FUNCTION event_private.valid_owned(smallint[]) FROM PUBLIC;
CREATE TABLE event_private.accounts (
 run_id bigint NOT NULL,
 kind smallint NOT NULL CHECK(kind IN(1,2)),
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 created_at bigint NOT NULL CHECK(created_at BETWEEN 0 AND 253402300799),
 balance bigint NOT NULL CHECK(kind=2 OR balance>=0),
 last_action_at bigint NOT NULL CHECK(last_action_at BETWEEN 0 AND 253402300799),
 holdings bigint[] NOT NULL DEFAULT array_fill(0::bigint,ARRAY[18]) CHECK(
  array_ndims(holdings)=1 AND array_lower(holdings,1)=1 AND cardinality(holdings)=18
  AND array_position(holdings,NULL) IS NULL AND 0<=ALL(holdings)
  AND (kind=1 OR holdings=array_fill(0::bigint,ARRAY[18]))),
 owned smallint[] NOT NULL DEFAULT '{}'::smallint[] CHECK(event_private.valid_owned(owned)
  AND (kind=2 OR cardinality(owned)=0)),
 PRIMARY KEY(run_id,actor_hash),
 FOREIGN KEY(run_id,kind) REFERENCES event_private.runs(id,kind) ON DELETE CASCADE
);
CREATE TABLE event_private.emotes (
 ordinal smallint PRIMARY KEY CHECK(ordinal BETWEEN 0 AND 165),
 name text UNIQUE NOT NULL CHECK(octet_length(name) BETWEEN 1 AND 64),
 kind smallint NOT NULL CHECK(kind IN(1,2,3)),
 value text NOT NULL CHECK(octet_length(value) BETWEEN 1 AND 64),
 width smallint NOT NULL CHECK(width BETWEEN 0 AND 250),
 height smallint NOT NULL CHECK(height BETWEEN 0 AND 250),
 source_id text UNIQUE NOT NULL CHECK(octet_length(source_id) BETWEEN 1 AND 64)
);
-- Release-owned metadata only. Artwork availability is a separate requirement.
INSERT INTO event_private.emotes VALUES
(0,'AngryWojak',2,'03d18964',27,32,'03d18964'),
(1,'Aquacry',2,'83cf2699',30,30,'83cf2699'),
(2,'AWOOOO',2,'e2ad2cb7',28,32,'e2ad2cb7'),
(3,'AYAYA',2,'f93f9e5a',32,31,'f93f9e5a'),
(4,'AYAYAHyper',2,'bc1ff2b8',32,32,'bc1ff2b8'),
(5,'BOOBA',2,'fb56168f',32,32,'fb56168f'),
(6,'BOOMER',2,'51ca59c2',32,32,'51ca59c2'),
(7,'Bruh',2,'67905c4f',32,32,'67905c4f'),
(8,'Catcry',2,'967f06c9',28,28,'967f06c9'),
(9,'ChadYes',2,'c7375c9d',31,32,'c7375c9d'),
(10,'COPIUM',2,'4dfb5c71',32,31,'4dfb5c71'),
(11,'DontBully',2,'199f7d0e',31,32,'199f7d0e'),
(12,'EZY',2,'adf2d2f0',28,26,'adf2d2f0'),
(13,'FeelsBadMan',2,'59b6bba6',30,29,'59b6bba6'),
(14,'FeelsGoodMan',2,'cda7b2fb',32,24,'cda7b2fb'),
(15,'FeelsOkayMan',2,'08b66b75',28,27,'08b66b75'),
(16,'FeelsSpecialMan',2,'25086889',28,25,'25086889'),
(17,'FeelsStrongMan',2,'0ee6ba1c',32,31,'0ee6ba1c'),
(18,'FeelsWeirdMan',2,'ad2977e6',28,27,'ad2977e6'),
(19,'gachiGASM',2,'c291d202',24,28,'c291d202'),
(20,'gachiHYPER',2,'634a21ba',24,28,'634a21ba'),
(21,'Gigachad',2,'7a95728b',29,32,'7a95728b'),
(22,'GoodNight',2,'a6d16707',31,32,'a6d16707'),
(23,'Hahaa',2,'af528e56',28,28,'af528e56'),
(24,'HeavyBreathing',2,'4623886c',32,32,'4623886c'),
(25,'KannaNom',2,'5de4addd',32,32,'5de4addd'),
(26,'KannaPolice',2,'98cf0be7',32,32,'98cf0be7'),
(27,'KEKW',2,'e54792d7',32,32,'e54792d7'),
(28,'KEKWait',2,'c2cfb2e3',32,32,'c2cfb2e3'),
(29,'MarisaFace',2,'857a9ea0',24,24,'857a9ea0'),
(30,'MeguminHappy',2,'a19762fc',32,32,'a19762fc'),
(31,'MikuStare',2,'b674048b',32,32,'b674048b'),
(32,'monkaChrist',2,'48c107b3',28,28,'48c107b3'),
(33,'monkaGIGA',2,'de27847b',28,28,'de27847b'),
(34,'monkaH',2,'acb11630',28,28,'acb11630'),
(35,'monkaHmm',2,'d3c674ba',32,32,'d3c674ba'),
(36,'monkaMEGA',2,'0b3318e4',28,28,'0b3318e4'),
(37,'monkaOMEGA',2,'bb299b4d',32,32,'bb299b4d'),
(38,'monkaS',2,'ed1cc57f',28,28,'ed1cc57f'),
(39,'monkaSpeed',2,'87c89650',32,30,'87c89650'),
(40,'monkaW',2,'b05923f5',32,32,'b05923f5'),
(41,'nepSmug',2,'57b01648',32,32,'57b01648'),
(42,'OMEGALUL',2,'c4035570',31,32,'c4035570'),
(43,'peepoBlanket',2,'099390a2',31,32,'099390a2'),
(44,'peepoClown',2,'8ea2d160',32,32,'8ea2d160'),
(45,'peepoHappy',2,'68104e2a',28,20,'68104e2a'),
(46,'peepoWTF',2,'3d8675e9',28,19,'3d8675e9'),
(47,'Pepega',2,'1ee7c5a1',32,25,'1ee7c5a1'),
(48,'PepeHands',2,'f2ecf801',32,32,'f2ecf801'),
(49,'PepeLaugh',2,'51cbf903',30,29,'51cbf903'),
(50,'PepeLmao',2,'25908e08',28,28,'25908e08'),
(51,'pepePoint',2,'90786369',32,32,'90786369'),
(52,'PepoG',2,'4459d60b',32,26,'4459d60b'),
(53,'pepoRope',2,'6ec0dd2c',32,31,'6ec0dd2c'),
(54,'PepoThink',2,'9ecd704b',32,31,'9ecd704b'),
(55,'pikachuS',2,'42faedcc',32,32,'42faedcc'),
(56,'PillowNo',2,'1e4d8dfa',32,32,'1e4d8dfa'),
(57,'PillowYes',2,'6f5bc7e5',32,32,'6f5bc7e5'),
(58,'Pog',2,'fad6951c',28,28,'fad6951c'),
(59,'POGGERS',2,'6f0d4e37',32,32,'6f0d4e37'),
(60,'PressF',2,'f0a256b9',32,30,'f0a256b9'),
(61,'REEeee',2,'b06b1566',32,32,'b06b1566'),
(62,'REEEEE',2,'ba70c4d9',32,28,'ba70c4d9'),
(63,'ReimuGlare',2,'bdf28159',32,32,'bdf28159'),
(64,'ReimuPalm',2,'41a37aa0',32,32,'41a37aa0'),
(65,'SadCatW',2,'d8f61d71',32,32,'d8f61d71'),
(66,'Sadge',2,'e024965e',28,22,'e024965e'),
(67,'SeetheWojak',2,'a9f848d3',28,32,'a9f848d3'),
(68,'Stonks',2,'53478ca5',31,32,'53478ca5'),
(69,'ThisIsFine',2,'d9bf8456',28,31,'d9bf8456'),
(70,'Thonk',2,'ec538b5c',32,27,'ec538b5c'),
(71,'TooLewd',2,'ddc55766',32,32,'ddc55766'),
(72,'Tuturu',2,'c72e8e84',32,32,'c72e8e84'),
(73,'umaruCry',2,'7242c342',28,28,'7242c342'),
(74,'WanWan',2,'8a527ac8',32,31,'8a527ac8'),
(75,'WeirdChamp',2,'3021a426',31,32,'3021a426'),
(76,'weSmart',2,'6476e57d',27,28,'6476e57d'),
(77,'wojakNPC',2,'24edafcc',32,32,'24edafcc'),
(78,'wojakWithered',2,'ed7d4c3a',32,30,'ed7d4c3a'),
(79,'WTFF',2,'8b7cc3e0',32,32,'8b7cc3e0'),
(80,'YEP',2,'e1899bbe',32,31,'e1899bbe'),
(81,'YesHoney',2,'2b414cf1',31,25,'2b414cf1'),
(82,'bane',3,'c458ef22',32,32,'c458ef22'),
(83,'bog',3,'c2e2602a',32,32,'c2e2602a'),
(84,'cia',3,'c69a1ef1',32,32,'c69a1ef1'),
(85,'cockmongler',3,'eda6f332',22,32,'eda6f332'),
(86,'desu',3,'80692b94',28,32,'80692b94'),
(87,'desusmirk',3,'72694e0e',41,32,'72694e0e'),
(88,'frodo',3,'e9d526e8',32,32,'e9d526e8'),
(89,'goldface',3,'7081142e',32,32,'7081142e'),
(90,'happycat',3,'1d3f2a13',27,32,'1d3f2a13'),
(91,'happyn',3,'afd49202',25,32,'afd49202'),
(92,'jannydog',3,'f0dcbf8a',35,32,'f0dcbf8a'),
(93,'koiwai',3,'1d7e369a',44,32,'1d7e369a'),
(94,'koiwaiwave',3,'0e313986',31,32,'0e313986'),
(95,'konata',3,'eb07a2c8',32,32,'eb07a2c8'),
(96,'laughingw',3,'6e6217c7',32,32,'6e6217c7'),
(97,'longcat',3,'0ee48fb4',37,32,'0ee48fb4'),
(98,'longcata',3,'95c37417',37,30,'95c37417'),
(99,'longcatb',3,'e77bc341',37,32,'e77bc341'),
(100,'moetron',3,'cf1d4b8d',32,32,'cf1d4b8d'),
(101,'mudkip',3,'a4b23eff',31,32,'a4b23eff'),
(102,'shoopdw',3,'11339e7b',23,32,'11339e7b'),
(103,'shoopdw2',3,'49bde730',100,32,'49bde730'),
(104,'troll',3,'d89a0070',37,32,'d89a0070'),
(105,'trollface',3,'7b4acfbf',32,26,'7b4acfbf'),
(106,'yaranaika',3,'a6955123',32,32,'a6955123'),
(107,'yaranaika2',3,'4d00227b',32,32,'4d00227b'),
(108,'happy',1,'&#x1F600;',0,0,'happy'),
(109,'grin',1,'&#x1F604;',0,0,'grin'),
(110,'xd',1,'&#x1F606;',0,0,'xd'),
(111,'grinsweat',1,'&#x1F605;',0,0,'grinsweat'),
(112,'rofl',1,'&#x1F923;',0,0,'rofl'),
(113,'lmao',1,'&#x1F602;',0,0,'lmao'),
(114,'smile',1,'&#x1F642;',0,0,'smile'),
(115,'wink',1,'&#x1F609;',0,0,'wink'),
(116,'glad',1,'&#x1F60A;',0,0,'glad'),
(117,'kiss',1,'&#x1F619;',0,0,'kiss'),
(118,'crazy',1,'&#x1F92A;',0,0,'crazy'),
(119,'think',1,'&#x1F914;',0,0,'think'),
(120,'wot',1,'&#x1F928;',0,0,'wot'),
(121,'kay',1,'&#x1F610;',0,0,'kay'),
(122,'yikes',1,'&#x1F612;',0,0,'yikes'),
(123,'eyeroll',1,'&#x1F644;',0,0,'eyeroll'),
(124,'confused',1,'&#x1F615;',0,0,'confused'),
(125,'pensive',1,'&#x1F614;',0,0,'pensive'),
(126,'disgust',1,'&#x1F922;',0,0,'disgust'),
(127,'vomit',1,'&#x1F92E;',0,0,'vomit'),
(128,'dizzy',1,'&#x1F635;',0,0,'dizzy'),
(129,'nerd',1,'&#x1F913;',0,0,'nerd'),
(130,'worry',1,'&#x1F61F;',0,0,'worry'),
(131,'sad',1,'&#x1F641;',0,0,'sad'),
(132,'frown',1,'&#x2639;&#xFE0F;',0,0,'frown'),
(133,'wow',1,'&#x1F632;',0,0,'wow'),
(134,'blush',1,'&#x1F633;',0,0,'blush'),
(135,'cry',1,'&#x1F622;',0,0,'cry'),
(136,'plead',1,'&#x1F97A;',0,0,'plead'),
(137,'baw',1,'&#x1F62D;',0,0,'baw'),
(138,'shock',1,'&#x1F631;',0,0,'shock'),
(139,'anguish',1,'&#x1F627;',0,0,'anguish'),
(140,'devil',1,'&#x1F608;',0,0,'devil'),
(141,'angry',1,'&#x1F620;',0,0,'angry'),
(142,'struggle',1,'&#x1F623;',0,0,'struggle'),
(143,'proud',1,'&#x1F624;',0,0,'proud'),
(144,'smirk',1,'&#x1F60F;',0,0,'smirk'),
(145,'drool',1,'&#x1F924;',0,0,'drool'),
(146,'love',1,'&#x1F60D;',0,0,'love'),
(147,'skull',1,'&#x1F480;',0,0,'skull'),
(148,'clown',1,'&#x1F921;',0,0,'clown'),
(149,'alien',1,'&#x1F47D;',0,0,'alien'),
(150,'robot',1,'&#x1F916;',0,0,'robot'),
(151,'ok',1,'&#x1F44C;',0,0,'ok'),
(152,'fu',1,'&#x1F595;',0,0,'fu'),
(153,'thup',1,'&#x1F44D;',0,0,'thup'),
(154,'thdown',1,'&#x1F44E;',0,0,'thdown'),
(155,'punch',1,'&#x1F44A;',0,0,'punch'),
(156,'pray',1,'&#x1F64F;',0,0,'pray'),
(157,'flex',1,'&#x1F4AA;',0,0,'flex'),
(158,'eyes',1,'&#x1F440;',0,0,'eyes'),
(159,'drip',1,'&#x1F4A6;',0,0,'drip'),
(160,'wind',1,'&#x1F4A8;',0,0,'wind'),
(161,'fire',1,'&#x1F525;',0,0,'fire'),
(162,'clover',1,'&#x1F340;',0,0,'clover'),
(163,'anger',1,'&#x1F4A2;',0,0,'anger'),
(164,'perfect',1,'&#x1F4AF;',0,0,'perfect'),
(165,'zzz',1,'&#x1F4A4;',0,0,'zzz');
CREATE TABLE event_private.catalog (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
INSERT INTO event_private.catalog VALUES(true,decode(repeat('00',32),'hex'));
REVOKE ALL ON ALL TABLES IN SCHEMA event_private FROM PUBLIC;
GRANT USAGE ON SCHEMA event_private,content TO board_event_owner;
GRANT SELECT ON event_private.runs,event_private.quotes,event_private.accounts,
 event_private.emotes,event_private.catalog TO board_event_owner;
GRANT UPDATE(account_count) ON event_private.runs TO board_event_owner;
-- PostgreSQL requires an UPDATE privilege for SELECT FOR SHARE. No public
-- function changes a quote; operator changes acquire the run lock first.
GRANT UPDATE(price) ON event_private.quotes TO board_event_owner;
GRANT INSERT ON event_private.accounts TO board_event_owner;
GRANT UPDATE(balance,last_action_at,holdings,owned) ON event_private.accounts TO board_event_owner;
GRANT EXECUTE ON FUNCTION event_private.valid_owned(smallint[]) TO board_event_owner;
GRANT CREATE ON SCHEMA event_private,content TO board_event_owner;
SET LOCAL ROLE board_event_owner;

CREATE FUNCTION event_private.catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT sha256(convert_to(jsonb_build_object(
 'role',(SELECT jsonb_build_array(rolname,rolcanlogin,rolsuper,rolcreatedb,rolcreaterole,
   rolreplication,rolbypassrls,rolinherit,rolconfig) FROM pg_roles WHERE rolname='board_event_owner'),
 'membership',(SELECT coalesce(jsonb_agg(jsonb_build_array(r.rolname,u.rolname,m.admin_option,m.inherit_option,m.set_option)
   ORDER BY r.rolname,u.rolname),'[]'::jsonb) FROM pg_auth_members m
   JOIN pg_roles r ON r.oid=m.roleid JOIN pg_roles u ON u.oid=m.member
   WHERE r.rolname='board_event_owner' OR u.rolname='board_event_owner'),
 'owner_grants',(SELECT jsonb_agg(value ORDER BY value::text) FROM (
   SELECT jsonb_build_array('relation',n.nspname,c.relname,a.privilege_type,a.is_grantable) AS value
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(c.relacl) a
    WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_event_owner')
   UNION ALL SELECT jsonb_build_array('column',n.nspname,c.relname,col.attname,a.privilege_type,a.is_grantable)
    FROM pg_attribute col JOIN pg_class c ON c.oid=col.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(col.attacl) a
    WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_event_owner')
   UNION ALL SELECT jsonb_build_array('function',n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),a.privilege_type,a.is_grantable)
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace,LATERAL aclexplode(p.proacl) a
    WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_event_owner')
   UNION ALL SELECT jsonb_build_array('schema',n.nspname,a.privilege_type,a.is_grantable)
    FROM pg_namespace n,LATERAL aclexplode(n.nspacl) a WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_event_owner')
   UNION ALL SELECT jsonb_build_array('database',a.privilege_type,a.is_grantable)
    FROM pg_database d,LATERAL aclexplode(d.datacl) a WHERE d.datname=current_database()
     AND a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_event_owner')
  ) grants),
 'default_acl',(SELECT coalesce(jsonb_agg(jsonb_build_array(pg_get_userbyid(d.defaclrole),n.nspname,d.defaclobjtype,d.defaclacl::text) ORDER BY pg_get_userbyid(d.defaclrole),n.nspname,d.defaclobjtype),'[]'::jsonb)
   FROM pg_default_acl d LEFT JOIN pg_namespace n ON n.oid=d.defaclnamespace
   WHERE d.defaclrole=(SELECT oid FROM pg_roles WHERE rolname='board_event_owner')
    OR EXISTS(SELECT 1 FROM aclexplode(d.defaclacl) a WHERE a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_event_owner'))),
 'schemas',(SELECT jsonb_agg(jsonb_build_array(n.nspname,pg_get_userbyid(n.nspowner),n.nspacl::text) ORDER BY n.nspname)
   FROM pg_namespace n WHERE n.nspname IN('event_private','content')),
 'relations',(SELECT jsonb_agg(jsonb_build_array(c.relname,c.relkind,pg_get_userbyid(c.relowner),c.relacl::text,
   c.relrowsecurity,c.relforcerowsecurity,
   (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attacl::text,
     pg_get_expr(d.adbin,d.adrelid)) ORDER BY a.attnum) FROM pg_attribute a
    LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
   (SELECT jsonb_agg(pg_get_constraintdef(k.oid) ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=c.oid),
   (SELECT jsonb_agg(pg_get_indexdef(i.indexrelid) ORDER BY ci.relname) FROM pg_index i
    JOIN pg_class ci ON ci.oid=i.indexrelid WHERE i.indrelid=c.oid),
   (SELECT jsonb_agg(pg_get_triggerdef(t.oid) ORDER BY t.tgname) FROM pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal))
    ORDER BY c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='event_private' AND c.relkind IN('r','v','m','p')),
 'functions',(SELECT jsonb_agg(jsonb_build_array(n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),
   pg_get_userbyid(p.proowner),p.proacl::text,pg_get_functiondef(p.oid)) ORDER BY n.nspname,p.proname,p.oid::regprocedure::text)
   FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE n.nspname='event_private' OR (n.nspname='content' AND (p.proname LIKE 'event\_%' ESCAPE '\' OR p.proname='events_ready'))),
 'metadata',(SELECT jsonb_agg(to_jsonb(e) ORDER BY e.ordinal) FROM event_private.emotes e)
 )::text,'UTF8'))
$$;
REVOKE ALL ON FUNCTION event_private.catalog_hash() FROM PUBLIC;

CREATE FUNCTION content.events_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT definition_hash=event_private.catalog_hash() FROM event_private.catalog WHERE singleton),false)
$$;
REVOKE ALL ON FUNCTION content.events_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.events_ready() TO board_public;

CREATE FUNCTION content.event_active_run(p_kind smallint)
RETURNS TABLE(id bigint,kind smallint,starts_at bigint,ends_at bigint)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 RETURN QUERY SELECT r.id,r.kind,r.starts_at,r.ends_at FROM event_private.runs r
 WHERE r.kind=p_kind AND r.enabled AND r.starts_at<=extract(epoch FROM statement_timestamp())
   AND r.ends_at>extract(epoch FROM statement_timestamp());
END $$;
REVOKE ALL ON FUNCTION content.event_active_run(smallint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_active_run(smallint) TO board_public;

CREATE FUNCTION event_private.market_json(p_balance bigint,p_holdings bigint[]) RETURNS jsonb
LANGUAGE sql IMMUTABLE STRICT SET search_path=pg_catalog,pg_temp AS $$
 SELECT jsonb_build_object('balance',p_balance)||coalesce((SELECT jsonb_object_agg(symbol,p_holdings[ordinal])
 FROM unnest(ARRAY['PEPE','WOJK','ANIME','CHAD','CLOWN','LOL','SICP','AUTSM','BANE','CIA','BOOB','RDDT','DESU','JANNY','GME','CHUCK','YTSB','GACHI'])
 WITH ORDINALITY stocks(symbol,ordinal) WHERE p_holdings[ordinal]>0),'{}'::jsonb)
$$;
REVOKE ALL ON FUNCTION event_private.market_json(bigint,bigint[]) FROM PUBLIC;

CREATE FUNCTION event_private.emote_json(p_start bigint,p_balance bigint,p_owned smallint[]) RETURNS jsonb
LANGUAGE sql STABLE STRICT SET search_path=pg_catalog,pg_temp AS $$
 SELECT jsonb_build_object('start_ts',p_start,'balance',p_balance,'owned',coalesce(
  (SELECT jsonb_agg(jsonb_build_array(e.name,e.kind,e.value) ORDER BY o.position)
   FROM unnest(p_owned) WITH ORDINALITY o(id,position) JOIN event_private.emotes e ON e.ordinal=o.id),'[]'::jsonb))
$$;
REVOKE ALL ON FUNCTION event_private.emote_json(bigint,bigint,smallint[]) FROM PUBLIC;

-- NULL means the run is closed or the requested opaque account is absent.
CREATE FUNCTION content.event_account(p_run bigint,p_actor bytea) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE a record;
BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN RAISE EXCEPTION 'Invalid event actor' USING ERRCODE='23514'; END IF;
 SELECT x.* INTO a FROM event_private.accounts x JOIN event_private.runs r ON r.id=x.run_id
 WHERE x.run_id=p_run AND x.actor_hash=p_actor AND r.enabled
  AND r.starts_at<=extract(epoch FROM statement_timestamp()) AND r.ends_at>extract(epoch FROM statement_timestamp());
 IF NOT FOUND THEN RETURN NULL; END IF;
 IF a.kind=1 THEN RETURN event_private.market_json(a.balance,a.holdings); END IF;
 RETURN event_private.emote_json(a.created_at,a.balance,a.owned);
END $$;
REVOKE ALL ON FUNCTION content.event_account(bigint,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_account(bigint,bytea) TO board_public;

-- Existing accounts remain accessible at capacity. No runtime caller can
-- change activation, capacity or an account's opaque identity.
CREATE FUNCTION content.event_open_account(p_run bigint,p_actor bytea,p_established boolean) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE r record; a record; t bigint; initial bigint;
BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_established IS NULL THEN RAISE EXCEPTION 'Invalid event actor' USING ERRCODE='23514'; END IF;
 SELECT * INTO r FROM event_private.runs WHERE id=p_run FOR UPDATE;
 t:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF NOT FOUND OR NOT r.enabled OR t<r.starts_at OR t>=r.ends_at THEN RETURN NULL; END IF;
 SELECT * INTO a FROM event_private.accounts WHERE run_id=p_run AND actor_hash=p_actor;
 IF NOT FOUND THEN
  IF r.account_count>=r.account_capacity THEN RETURN jsonb_build_object('error','Event account capacity reached'); END IF;
  initial:=CASE WHEN r.kind=1 THEN 1000 WHEN p_established THEN 100 ELSE 0 END;
  INSERT INTO event_private.accounts(run_id,kind,actor_hash,created_at,balance,last_action_at)
   VALUES(p_run,r.kind,p_actor,t,initial,t) RETURNING * INTO a;
  UPDATE event_private.runs SET account_count=account_count+1 WHERE id=p_run;
 END IF;
 IF a.kind=1 THEN RETURN event_private.market_json(a.balance,a.holdings); END IF;
 RETURN event_private.emote_json(a.created_at,a.balance,a.owned);
END $$;
REVOKE ALL ON FUNCTION content.event_open_account(bigint,bytea,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_open_account(bigint,bytea,boolean) TO board_public;
CREATE FUNCTION content.event_market_trade(p_run bigint,p_actor bytea,p_side smallint,
 p_stock smallint,p_amount integer,p_price bigint) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE r record; a record; current_price bigint; cost bigint; t bigint; next_balance bigint; next_holdings bigint[];
BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN RAISE EXCEPTION 'Invalid event actor' USING ERRCODE='23514'; END IF;
 SELECT * INTO r FROM event_private.runs WHERE id=p_run AND kind=1 FOR SHARE;
 t:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF NOT FOUND OR NOT r.enabled OR t<r.starts_at OR t>=r.ends_at THEN RETURN NULL; END IF;
 IF p_side IS NULL OR p_side NOT IN(1,2) THEN RETURN jsonb_build_object('error','Bad request'); END IF;
 IF p_stock IS NULL OR p_stock NOT BETWEEN 0 AND 17 THEN RETURN jsonb_build_object('error','Stock not found'); END IF;
 IF p_amount IS NULL OR p_amount NOT BETWEEN 1 AND 1000 THEN RETURN jsonb_build_object('error','Invalid amount'); END IF;
 IF p_price IS NULL OR p_price<=0 THEN RETURN jsonb_build_object('error','Invalid price'); END IF;
 SELECT price INTO current_price FROM event_private.quotes WHERE run_id=p_run AND stock=p_stock FOR SHARE;
 IF NOT FOUND THEN RETURN jsonb_build_object('error','Invalid price'); END IF;
 IF current_price<>p_price THEN RETURN jsonb_build_object('error','The price has changed.'); END IF;
 IF current_price>9223372036854775807::bigint/p_amount THEN RETURN jsonb_build_object('error','Invalid amount'); END IF;
 cost:=current_price*p_amount;
 SELECT * INTO a FROM event_private.accounts WHERE run_id=p_run AND actor_hash=p_actor FOR UPDATE;
 IF NOT FOUND THEN RETURN jsonb_build_object('error','Account not found'); END IF;
 t:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF t<r.starts_at OR t>=r.ends_at THEN RETURN NULL; END IF;
 IF (p_side=1 AND a.balance<cost) OR (p_side=2 AND a.holdings[p_stock+1]<p_amount) THEN
  RETURN jsonb_build_object('error','Your account balance is too low'); END IF;
 IF a.last_action_at>t-10 THEN RETURN jsonb_build_object('error','You can only make an order once every 10 seconds'); END IF;
 next_holdings:=a.holdings;
 IF p_side=1 THEN
  IF a.holdings[p_stock+1]>9223372036854775807::bigint-p_amount THEN RETURN jsonb_build_object('error','Invalid amount'); END IF;
  next_balance:=a.balance-cost;
  next_holdings[p_stock+1]:=next_holdings[p_stock+1]+p_amount;
 ELSE
  IF a.balance>9223372036854775807::bigint-cost THEN RETURN jsonb_build_object('error','Invalid amount'); END IF;
  next_balance:=a.balance+cost;
  next_holdings[p_stock+1]:=next_holdings[p_stock+1]-p_amount;
 END IF;
 UPDATE event_private.accounts SET balance=next_balance,holdings=next_holdings,last_action_at=t
  WHERE run_id=p_run AND actor_hash=p_actor;
 RETURN event_private.market_json(next_balance,next_holdings);
END $$;
REVOKE ALL ON FUNCTION content.event_market_trade(bigint,bytea,smallint,smallint,integer,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_market_trade(bigint,bytea,smallint,smallint,integer,bigint) TO board_public;

CREATE FUNCTION content.event_emote_acquire(p_run bigint,p_actor bytea,p_id text,p_roll smallint) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE r record; a record; e record; t bigint; cost bigint; available numeric;
 next_balance bigint; next_owned smallint[]; obtained jsonb:='[]'::jsonb;
 refund integer:=0; draw integer; chosen_kind smallint; pool_count integer; chosen_offset integer; i integer;
BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN RAISE EXCEPTION 'Invalid event actor' USING ERRCODE='23514'; END IF;
 SELECT * INTO r FROM event_private.runs WHERE id=p_run AND kind=2 FOR SHARE;
 t:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF NOT FOUND OR NOT r.enabled OR t<r.starts_at OR t>=r.ends_at THEN RETURN NULL; END IF;
 IF p_roll IS NULL OR p_roll NOT IN(0,3,10) OR (p_roll>0 AND p_id IS NOT NULL) THEN
  RETURN jsonb_build_object('status','error','msg','Bad Request'); END IF;
 IF p_roll=0 THEN
  IF p_id IS NULL OR octet_length(p_id)>64 THEN RETURN jsonb_build_object('status','error','msg','Bad Request'); END IF;
  SELECT * INTO e FROM event_private.emotes WHERE source_id=p_id;
  IF NOT FOUND THEN RETURN jsonb_build_object('status','error','msg','Bad Request'); END IF;
  cost:=CASE e.kind WHEN 1 THEN 50 WHEN 2 THEN 150 ELSE 300 END;
 ELSE cost:=p_roll*10;
 END IF;
 SELECT * INTO a FROM event_private.accounts WHERE run_id=p_run AND actor_hash=p_actor FOR UPDATE;
 IF NOT FOUND THEN RETURN jsonb_build_object('status','error','msg','Bad Request'); END IF;
 t:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF t<r.starts_at OR t>=r.ends_at THEN RETURN NULL; END IF;
 IF p_roll=0 THEN
  IF e.ordinal=ANY(a.owned) THEN RETURN jsonb_build_object('status','error','msg','You already have that emote'); END IF;
 END IF;
 IF t<a.created_at THEN RETURN jsonb_build_object('status','error','msg','Invalid event time'); END IF;
 available:=floor((t::numeric-a.created_at)/300)*20+a.balance;
 IF available<cost THEN RETURN jsonb_build_object('status','error','msg','Not enough points','pts',a.balance); END IF;
 IF available>9223372036854775807::numeric THEN RETURN jsonb_build_object('status','error','msg','Invalid event balance'); END IF;
 next_owned:=a.owned;
 IF p_roll=0 THEN
  next_owned:=array_append(next_owned,e.ordinal);
  obtained:=jsonb_build_array(e.name,e.kind,e.value);
 ELSE
  -- All draws happen inside the server transaction. No endpoint or SQL
  -- parameter can submit a desired lottery result. Source pool weights remain
  -- 50/40/10; the literal roster has 58/82/26 entries, not its older comments.
  FOR i IN 1..p_roll LOOP
   draw:=floor(random()*100)::integer;
   chosen_kind:=CASE WHEN draw<50 THEN 1 WHEN draw<90 THEN 2 ELSE 3 END;
   SELECT count(*) INTO pool_count FROM event_private.emotes WHERE kind=chosen_kind;
   chosen_offset:=floor(random()*pool_count)::integer;
   SELECT * INTO e FROM event_private.emotes WHERE kind=chosen_kind ORDER BY ordinal LIMIT 1 OFFSET chosen_offset;
   IF NOT FOUND THEN RAISE EXCEPTION 'Event roster changed' USING ERRCODE='23514'; END IF;
   IF e.ordinal=ANY(next_owned) THEN refund:=refund+CASE WHEN e.kind=3 THEN 10 ELSE 5 END;
   ELSE next_owned:=array_append(next_owned,e.ordinal);
   END IF;
   obtained:=obtained||jsonb_build_array(jsonb_build_array(e.name,e.kind,e.value));
  END LOOP;
 END IF;
 IF a.balance::numeric+refund-cost NOT BETWEEN -9223372036854775808::numeric AND 9223372036854775807::numeric THEN
  RETURN jsonb_build_object('status','error','msg','Invalid event balance'); END IF;
 next_balance:=(a.balance::numeric+refund-cost)::bigint;
 UPDATE event_private.accounts SET balance=next_balance,owned=next_owned,last_action_at=t
  WHERE run_id=p_run AND actor_hash=p_actor;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('obtained',obtained,'balance',next_balance));
END $$;
REVOKE ALL ON FUNCTION content.event_emote_acquire(bigint,bytea,text,smallint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_emote_acquire(bigint,bytea,text,smallint) TO board_public;
CREATE FUNCTION content.event_market_quotes(p_run bigint) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE prices jsonb;
BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 IF NOT EXISTS(SELECT 1 FROM event_private.runs r WHERE r.id=p_run AND r.kind=1 AND r.enabled
  AND r.starts_at<=extract(epoch FROM statement_timestamp()) AND r.ends_at>extract(epoch FROM statement_timestamp())) THEN RETURN NULL; END IF;
 SELECT jsonb_object_agg(s.symbol,q.price) INTO prices FROM event_private.quotes q
 JOIN unnest(ARRAY['PEPE','WOJK','ANIME','CHAD','CLOWN','LOL','SICP','AUTSM','BANE','CIA','BOOB','RDDT','DESU','JANNY','GME','CHUCK','YTSB','GACHI']) WITH ORDINALITY s(symbol,ordinal)
  ON s.ordinal=q.stock+1 WHERE q.run_id=p_run;
 RETURN coalesce(prices,'{}'::jsonb);
END $$;
REVOKE ALL ON FUNCTION content.event_market_quotes(bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_market_quotes(bigint) TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA event_private,content FROM board_event_owner;
SET LOCAL ROLE board_event_owner;
SELECT set_config('board.event_catalog_hash',encode(event_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE event_private.catalog SET definition_hash=decode(current_setting('board.event_catalog_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority initialization failed'; END IF;
END $$;

RESET ROLE;

-- Only the two catalogs that include content's namespace ACL change here.
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.event_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.event_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.event_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.event_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_vip_ready() IS DISTINCT FROM true
 OR content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Event authority changed unrelated posting authority'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event report authority refresh failed'; END IF; END $$;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority changed restriction authority'; END IF; END $$;
RESET ROLE;
