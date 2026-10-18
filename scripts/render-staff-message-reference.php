<?php
// Render the pinned source view with plain synthetic messages. Its source
// image-embedding formatter is deliberately outside this fixture's scope.
date_default_timezone_set('UTC');
define('IN_APP', true);
$view = file_get_contents(__DIR__.'/../fixtures/staff-messages-source.php');
if (hash('sha256', $view) !== '39d9da9ce798e7ac131d84e5a792c06d620553abecc245c40f0d0932ea7c8ad4') {
    throw new RuntimeException('Source staff-message view differs.');
}
class StaffMessageReference {
    const DATE_FORMAT = 'm/d/y H:i';
    public $offset = 0;
    public $previous_offset = 0;
    public $next_offset = 0;
    public $ts = false;
    public $messages = [
        ['created_on'=>1700000003,'boards'=>'msgtwo,msgone','content'=>'Owned mixed-board message'],
        ['created_on'=>1700000001,'boards'=>'msgone','content'=>'Owned allowed message'],
        ['created_on'=>1700000000,'boards'=>'','content'=>'Owned general message'],
    ];
    public function format_staff_message($message) { return $message; }
    public function render($view) {
        ob_start();
        eval('?>'.$view);
        return ob_get_clean();
    }
}
$scenarios = [];
foreach (['unread'=>false, 'mixed'=>1700000001, 'read'=>2000000000, 'empty'=>false] as $name=>$seen) {
    $page = new StaffMessageReference();
    $page->ts = $seen;
    if ($name === 'empty') {
        $page->messages = [];
        $page->offset = 25;
    }
    $scenarios[$name] = $page->render($view);
}
$rendered = $scenarios['unread'];
$path = __DIR__.'/../fixtures/staff-messages-reference.html';
if (in_array('--write', $argv, true)) {
    file_put_contents($path, $rendered);
} elseif (file_get_contents($path) !== $rendered) {
    throw new RuntimeException('Source staff-message reference differs.');
}
$casesPath = __DIR__.'/../fixtures/staff-messages-pages.json';
$encoded = json_encode($scenarios, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR)."\n";
if (in_array('--write', $argv, true)) file_put_contents($casesPath, $encoded);
elseif (file_get_contents($casesPath) !== $encoded) throw new RuntimeException('Source staff-message page states differ.');
echo "Source staff-message references match four read/navigation states.\n";
