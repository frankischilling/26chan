<?php
// Isolated original-page oracle: selected, hash-checked functions and owned rows.
// No source request handler, database connection or filesystem writer is loaded.
$input = json_decode(file_get_contents($argv[1]), true, 512, JSON_THROW_ON_ERROR);
foreach ($input['functions'] as $code) eval($code);
$baseline = $input['baseline'];
$valid_boards = implode('|', $baseline['allowlist']);
foreach([
 'SQLLOG'=>'g','BOARD_DIR'=>'g','TITLE'=>'/g/ - Owned source links','META_DESCRIPTION'=>'Owned synthetic comments','META_KEYWORDS'=>'','META_ROBOTS'=>'noindex',
 'NAV_TXT'=>'owned-top','NAV2_TXT'=>'owned-bottom','STATIC_SERVER'=>'/source-assets/','STATIC_IMG_DIR2'=>'/source-assets/image/',
 'RES_DIR'=>'owned/','RES_DIR2'=>'thread/','PHP_EXT'=>'.html','PHP_EXT2'=>'', 'SELF_PATH_ABS'=>'/g/imgboard.php',
 'EMBEDEARLY'=>'','AD_EMBEDEARLY'=>'','EMBEDLATE'=>'','AD_ADBLOCK_TEXT'=>'',
 'S_RETURN'=>'Return','S_CATALOG'=>'Catalog','S_TOP'=>'Top','S_BOTTOM'=>'Bottom','S_REFRESH'=>'Refresh','S_FORM_REPLY'=>'Post a Reply',
 'S_FORM_THREAD'=>'Start a New Thread','S_REPDEL'=>'Delete Post:','S_DELPICONLY'=>'File Only','S_DELETE'=>'Delete',
 'AD_MIDDLE_ENABLE'=>0,'S_THREAD_CLOSED'=>'Thread closed.<br>You may not reply at this time.',
 'S_REPLY'=>'Reply','S_ANONAME'=>'Anonymous',
 'CSS_VERSION'=>716,'JS_VERSION'=>1191,'JS_VERSION_CORE'=>1128,'JS_VERSION_EXT'=>1191,
 'MAX_COM_CHARS'=>2000,'MAX_KB'=>4096,'MAX_WEBM_FILESIZE'=>4096,'MAX_LINES'=>70,'DEF_PAGES'=>1,
 'RENZOKU'=>0,'RENZOKU2'=>0,'RENZOKU3'=>0,'DEFAULT_BURICHAN'=>1,'NEW_HTML'=>true,
 'JANITOR_BOARD'=>0,'TITLE_IMAGE_TYPE'=>0,'TEST_BOARD'=>0,'SHOW_COUNTRY_FLAGS'=>0,'ENABLE_BOARD_FLAGS'=>0,
 'CODE_TAGS'=>0,'SJIS_TAGS'=>0,'USE_RSS'=>0,'RTA'=>0,'SHOW_THREAD_UNIQUES'=>0,'DISP_ID'=>0,'JSMATH'=>0,
 'SPOILERS'=>0,'CAPTCHA_TWISTER'=>0,'AD_PLEA'=>0,'ENABLE_ARCHIVE'=>0,'ENABLE_PAINTERJS'=>0,
 'TEXT_ONLY'=>0,'IS_REBUILDD'=>1,'STATIC_REBUILD'=>0,'ENABLE_JSON'=>0,'PAGE_MAX'=>0,'PARTY'=>0,
 'META_BOARD'=>0,'FORCED_ANON'=>0,'FORCE_COM'=>0,
] as $name=>$value) define($name,$value);
// Only the controlled ASCII fixture enters these missing legacy-runtime helpers.
function mb_strlen($s) { if(preg_match('/[^\x00-\x7f]/',$s)) throw new RuntimeException('ASCII only'); return strlen($s); }
function each(&$a) { $key=key($a); if($key===null) return false; $value=current($a); next($a); return [1=>$value,'value'=>$value,0=>$key,'key'=>$key]; }
function file_get_contents_cached($key) { if(!in_array($key,['owned-top','owned-bottom'],true)) throw new RuntimeException('Unexpected file'); return '<div class="boardList desktop">[<a href="/g/">g</a>]</div>'; }
function log_cache(...$args) {}
function logtime($text) {}
function get_json_tail_size($id) { return 0; }
function style_group() { return 'ws_style'; }
function generate_page_metatags(...$args) { return false; }
function generate_page_title(...$args) { return 'Owned source links'; }
function form_ads(&$html) {}
function get_contest_banner() { return ''; }
function display_uid($id,$capcode) { return ''; }
function display_no($id) { return (string)$id; }
function rebuildd_stats() { return ''; }
function print_page($path,$html) { if($path!=='owned/100.html') throw new RuntimeException('Unexpected write'); $GLOBALS['captured']=$html; }
class L { static function d($board) { return 'example.test'; } }
function mysql_global_call($query) { return $GLOBALS['baseline']['allowlist']; }
function mysql_column_array($rows) { return $rows; }
function mysql_board_call($format,$board,$no) { if(!in_array($format,['SELECT resto FROM `%s` WHERE no=%d','select resto from `%s` where no=%d'],true)) throw new RuntimeException('Unexpected lookup'); return $board==='g' && (int)$no===123 ? 100 : false; }
function mysql_num_rows($rows) { return $rows===false?0:1; }
function mysql_fetch_row($rows) { return [$rows]; }
function mysql_result($rows,$index) { return $rows; }
$base=['no'=>100,'resto'=>0,'name'=>'Anonymous','sub'=>'Owned source links','com'=>'Owned source link comparison','capcode'=>'none','ext'=>'','id'=>'','country'=>'','board_flag'=>'','since4pass'=>0,'sticky'=>0,'closed'=>1,'archived'=>0,'semantic_url'=>'','bumplimit'=>0,'imagelimit'=>0,'replycount'=>1,'children'=>[123=>0],'time'=>1788868800,'now'=>'09/08/26(Tue)08:00:00'];
$raw=">>123 >>000123 >>>/g/+123 >>>/g/123e0\n>>999 >>>/g/-123 >>>/g/1e309 >>>/unknown/+123\nhttps://example.test/a?a=1&b=2 https://www.4chan.org/faq\nentityneedle & <tag> \"quoted\" 'apostrophe' &amp; &#39;";
$comment=normalize_and_linkify(htmlspecialchars($raw,ENT_QUOTES));
$comment=str_replace("\n",'<br>',$comment);
$log=[100=>$base,123=>array_merge($base,['no'=>123,'resto'=>100,'sub'=>'','com'=>$comment,'time'=>1788869100,'now'=>'09/08/26(Tue)08:05:00'])];
$_SERVER['REQUEST_METHOD']='POST'; $mode='nothing';
updatelog_real(100,1);
if(empty($captured)) throw new RuntimeException('No captured page');
echo $captured;
