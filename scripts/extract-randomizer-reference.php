<?php
// Read the pinned posting pattern and execute only its isolated dice block and
// pure truncation function on bounded synthetic data. Never load the old app.
if ($argc < 3) { fwrite(STDERR, "Usage: php scripts/extract-randomizer-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$source = file_get_contents($root . '/imgboard.php');
if (!preg_match('/preg_match\( "(\/dice[^"\n]+)"/', $source, $match)) { throw new RuntimeException('Dice pattern is missing.'); }
$pattern = stripcslashes($match[1]);
$start = strpos($source, "\tif (DICE_ROLL == 1) {");
$end = strpos($source, "\t// fixme: this is needed for bypassing the r9k filter.", $start);
if ($start === false || $end === false || $end - $start > 3000) { throw new RuntimeException('Dice block is missing or changed.'); }
$block = substr($source, $start, $end - $start);
define('DICE_ROLL', 1);
define('S_DICE_PFX', 'Rolled');
define('SJIS_TAGS', false);
function load_truncation($source) {
    $tokens = token_get_all(preg_replace('/^<\?(?=\s)/', '<?php', $source));
    for ($i = 0; $i < count($tokens); ++$i) {
        if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
        $start = $i;
        do { ++$i; } while (is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
        if (!is_array($tokens[$i]) || $tokens[$i][1] !== 'truncate_comment') { continue; }
        $depth = 0; $body = ''; $opened = false;
        for ($j = $start; $j < count($tokens); ++$j) {
            $token = $tokens[$j]; $body .= is_array($token) ? $token[1] : $token;
            if ($token === '{') { ++$depth; $opened = true; }
            if ($token === '}' && --$depth === 0 && $opened) { eval($body); return; }
        }
    }
    throw new RuntimeException('Truncation function is missing.');
}
load_truncation($source);
$cases = [];
foreach (['dice+2d1+3', 'dice+2d1-3', 'dice+2d1 -3', 'dice+2d1--3', 'dice 1 1', 'prefix dice+26+1 suffix', 'SaGedice+2d1+3', 'DICE+2d1', 'dice++2d1'] as $input) {
    $email = str_ireplace('sage', '', $input);
    $com = 'ordinary'; $dicesum = 0; $diceadd_formatted = '';
    // Optional PCRE groups are absent in PHP 8; initialize the source's expected
    // default values without changing its pattern or generated text.
    $match = array_fill(0, 6, '');
    set_error_handler(function($number, $message) { if (str_contains($message, 'Undefined array key')) { return true; } return false; });
    eval($block);
    restore_error_handler();
    $dice = str_starts_with($com, '<b>') ? substr($com, 3, strpos($com, '<br>') - 3) : null;
    $cases[] = ['options' => $input, 'dice' => $dice];
}
if (!preg_match('/\$fortunes\s*=\s*(array\([^\n]+\));/', $source, $match)) { throw new RuntimeException('Standard fortunes are missing.'); }
$fortunes = eval('return ' . $match[1] . ';');
$palette = [];
foreach ($fortunes as $index => $text) {
    $color = '#' . sprintf('%02x%02x%02x', 127 + 127 * sin(2 * M_PI * $index / count($fortunes)), 127 + 127 * sin(2 * M_PI * $index / count($fortunes) + 2 / 3 * M_PI), 127 + 127 * sin(2 * M_PI * $index / count($fortunes) + 4 / 3 * M_PI));
    $palette[] = ['text' => $text, 'color' => $color];
}
$teasers = [];
foreach ([['dice' => $cases[0]['dice'], 'fortune' => null, 'color' => null], ['dice' => null, 'fortune' => $palette[0]['text'], 'color' => $palette[0]['color']]] as $metadata) {
    foreach (['ordinary', str_repeat('x', 301)] as $comment) {
        foreach ([false, true] as $truncate) {
            $stored = htmlspecialchars($comment, ENT_QUOTES, 'UTF-8');
            if ($metadata['dice'] !== null) { $stored = '<b>' . $metadata['dice'] . '<br><br></b>' . $stored; }
            if ($metadata['fortune'] !== null) { $stored .= '<span class="fortune" style="color:' . $metadata['color'] . '"><br><br><b>Your fortune: ' . $metadata['fortune'] . '</b></span>'; }
            $converted = preg_replace('#(<br>)+#', ' ', $stored);
            $teaser = $truncate ? truncate_comment($converted, 300, true) : strip_tags($converted, '<s>');
            $teasers[] = $metadata + ['comment' => $comment, 'truncate' => $truncate, 'teaser' => $teaser];
        }
    }
}
$json = json_encode(['reference' => 'operator-supplied 4chan-old checkout', 'files' => ['imgboard.php' => hash('sha256', $source), 'catalog.php' => hash_file('sha256', $root . '/catalog.php')], 'dice_cases' => $cases, 'fortunes' => $palette, 'teaser_cases' => $teasers], JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Randomizer reference differs.'); }
    echo 'Randomizer reference matches 9 dice cases, 13 fortunes and 8 teasers.' . "\n";
} else { file_put_contents($argv[2], $json); }
