<?php
// Execute only the hash-pinned notice block. SQL and report effects are recorded.
if (PHP_SAPI !== 'cli' || PHP_VERSION !== '8.4.24' || PHP_INT_SIZE !== 8
 || $argc < 3 || $argc > 4 || ($argc === 4 && $argv[3] !== '--check')) {
 throw new RuntimeException('Use 64-bit PHP 8.4.24: SOURCE OUTPUT [--check]');
}
$root=realpath($argv[1]); $parent=realpath(dirname($argv[2])); $existing=realpath($argv[2]);
if (!$root || !$parent || $parent===$root || str_starts_with($parent,$root.'/')
 || ($existing && str_starts_with($existing,$root.'/'))) {throw new RuntimeException('Output must be outside reference source.');}
$source=file_get_contents($root.'/admin.php');
$strings=file_get_contents($root.'/config/global_strings.ini');
$sourceHash='4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb';
$stringsHash='3b3b60083db74fb1693af6f0824cccd6ec3ca446242474008cf9be474c65cd17';
if (hash('sha256',$source)!==$sourceHash || hash('sha256',$strings)!==$stringsHash) {throw new RuntimeException('Audited notice source differs.');}
foreach (['S_USERWARNEDFORPOST','S_USERBANNEDFORPOST'] as $name) {
 if (preg_match('/^'.preg_quote($name,'/').' = ([^\r\n]+)\r?$/m',$strings,$match)!==1) {throw new RuntimeException('Missing notice string.');}
 define($name,$match[1]);
}
$start=strpos($source,"\tif( \$banmsg ) {");
$end=strpos($source,"\t//print ",$start);
if ($start===false || $end===false || $end-$start>4096) {throw new RuntimeException('Notice boundary differs.');}
$body=substr($source,$start,$end-$start);
define('SQLLOG','owned'); define('BOARD_DIR','owned'); define('S_SQLFAIL','SQL failure');
function has_flag($name) {global $flag; if ($name!=='banmsg') {throw new RuntimeException('Unexpected flag.');} return $flag;}
// Parameterization replaces SQL escaping natively. Capture the already-escaped
// HTML selection here without executing or reconstructing an SQL string.
function mysql_real_escape_string($text) {return $text;}
function mysql_board_call(...$args) {global $writes; $writes++; return true;}
function admin_clear_reports($board,$post) {global $cleared; if ($board!=='owned'||$post!==17) {throw new RuntimeException('Unexpected clear target.');} $cleared=true;}
$cases=[];
foreach ([false,true] as $banmsg) foreach ([false,true] as $warn)
foreach ([false,true] as $is_manager) foreach ([false,true] as $flag)
foreach (['','delpost','delfile','move','archive','close','permasage','delall','delallrep'] as $action)
foreach ([null,'','0',' ','Owned message','<script>"x"</script> & \'quote\'','&amp; already','a\\b',"line\nnext",'猫'] as $custom) {
 $_POST=['postban'=>$action]; if ($custom!==null) {$_POST['custombanmsg']=$custom;}
 $_SERVER=['REQUEST_TIME'=>1000]; $postid=17; $resto=10; $writes=0; $cleared=false; $samessage=null;
 eval($body);
 $cases[]=['enabled'=>$banmsg,'warning'=>$warn,'manager'=>$is_manager,'flag'=>$flag,'action'=>$action,'custom'=>$custom,
  'html'=>$samessage===null?null:' <br><br><strong style="color: red;">('.$samessage.')</strong>',
  'writes'=>$writes,'clear'=>$cleared];
}
$json=json_encode(['source_sha256'=>$sourceHash,'strings_sha256'=>$stringsHash,'scope'=>'Isolated notice selection and recorded update/auto-clear effects; no database or staff authority claim.','cases'=>$cases],JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR)."\n";
if (($argv[3]??'')==='--check') {if(file_get_contents($argv[2])!==$json){throw new RuntimeException('Notice fixture differs.');}}
else {file_put_contents($argv[2],$json);}
