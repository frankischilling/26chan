<?php
// Evaluate only three audited pure functions, never the application or its includes.
if ($argc < 3) {
    fwrite(STDERR, "Usage: php scripts/extract-admission-normalization-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
if (!extension_loaded('intl') || !extension_loaded('mbstring')) {
    throw new RuntimeException('The fixture requires ICU and UTF-8 support.');
}
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$source = file_get_contents($root . '/lib/postfilter.php');
$hash = 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207';
if (hash('sha256', $source) !== $hash) {
    throw new RuntimeException('Audited admission source differs.');
}
$names = ['normalize_ascii', 'strip_zerowidth', 'normalize_text'];
$found = [];
$pure = '';
$tokens = token_get_all($source);
for ($i = 0; $i < count($tokens); ++$i) {
    if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
    $start = $i;
    do { ++$i; } while (is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
    if (!is_array($tokens[$i]) || !in_array($tokens[$i][1], $names, true)) { continue; }
    $name = $tokens[$i][1];
    if (isset($found[$name])) { throw new RuntimeException('Repeated audited function.'); }
    $depth = 0;
    $opened = false;
    $body = '';
    for ($j = $start; $j < count($tokens); ++$j) {
        $token = $tokens[$j];
        $body .= is_array($token) ? $token[1] : $token;
        if ($token === '{') { ++$depth; $opened = true; }
        if ($token === '}' && --$depth === 0 && $opened) { break; }
    }
    if ($depth !== 0 || !$opened || strlen($body) > 4096) {
        throw new RuntimeException('Audited pure function exceeds its bounds.');
    }
    $pure .= $body . "\n";
    $found[$name] = true;
}
if (count($found) !== count($names) || strlen($pure) > 8192) {
    throw new RuntimeException('Audited pure-function set is incomplete.');
}
eval($pure);

$inputs = [
    '', 'An ordinary paper lighthouse.',
    'HTTP://example.invalid/Paper?Name=Fold&Width=4/2;~_-,.:=',
    'first (dot) invalid [DoT] example {DOT} test =dot=',
    '(dot] [dot} {dot) =dot] ( dot ) DOT dot',
    "A\r\nB\tC D &gt; &amp; &#35; # ## + ! @ $ % ^ * ( ) [ ] { } ",
    '[spoiler]Paper[/spoiler] [code]Code[/code] [sjis]Art[/sjis]',
    'Übermäßig déjà vu Æsir Œuvre Łódź İstanbul ǅak ẞ ß',
    "e\u{0301} o\u{0308} A\u{030A} A\u{034F}\u{0301}",
    'Ελληνικά ΑΘΗΝΑ Σίσυφος γγ γκ γχ γξ αυ ευ ου αι',
    'Русский Москва Щука Подъезд Ь ь Ъ ъ Ц Ш Ы Ю Я',
    'Հայերեն Երևան ქართული თბილისი',
    'العربية مرحبا السلام عليكم لا ﻻ ﷲ',
    'עברית ירושלים שלום עולם',
    '中文 東京 北京 重庆 重慶 藏文 沈阳 秘鲁 西安 长安',
    'ひらがな こんにちは カタカナ コンニチハ きゃ っし んあ んや',
    '한국어 서울 대한민국 한글 한글',
    'ภาษาไทย กรุงเทพมหานคร สวัสดีครับ ขอบคุณภาษาไทย',
    'ไทยภาษาไทย กระดาษพับ ดอกไม้ วันนี้',
    'हिन्दी संस्कृत भारत कागज़ नमस्ते',
    'বাংলা ਪੰਜਾਬੀ ગુજરાતી ଓଡ଼ିଆ தமிழ் తెలుగు ಕನ್ನಡ മലയാളം',
    'සිංහල မြန်မာဘာသာ ދިވެހި አማርኛ ܣܘܪܝܝܐ',
    'ᐃᓄᒃᑎᑐᑦ ㄅㄆㄇㄈ ᚠᚢᚦᚨ 𐐀𐐁𐐂',
    'ＡＢＣ ａｂｃ １２３ ①②③ ⅣⅤⅥ ﬃ ﬁ ℌ ℍ K Å',
    'ｶﾀｶﾅ ｶﾞｯｺｳ ﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ',
    'AΕBРC中DあE한FภาษาไทยG العربيةH',
    'A Ελληνικά / Русский : 中文 - العربية & ภาษาไทย Z',
    "α\u{0301}\u{200D}а\u{0301}\u{200C}中\u{FE0F}A",
    'Paper😀fold🧩☀☂🚀🛰️🪁',
];
foreach (['(', '[', '=', '{'] as $open) {
    foreach ([')', ']', '=', '}'] as $close) {
        foreach (['dot', 'DOT', 'dOt'] as $middle) {
            $inputs[] = 'A' . $open . $middle . $close . 'B';
        }
    }
}
// Scalar cases catch case-preserving transliteration and compatibility forms.
foreach ([[0, 127], [0x80, 0x24F], [0x370, 0x52F], [0x0E00, 0x0E7F],
          [0x1E00, 0x1EFF], [0xFF00, 0xFFEF]] as [$start, $end]) {
    for ($point = $start; $point <= $end; ++$point) {
        $inputs[] = 'A' . mb_chr($point, 'UTF-8') . 'z';
    }
}
foreach ([0x1, 0x8, 0xE, 0xF, 0x10, 0x1F, 0x7F, 0x8D, 0x9F, 0xA0, 0xAD,
          0x34F, 0x702, 0x115F, 0x1160, 0x11A6, 0x17B4, 0x17B5,
          0x180B, 0x180C, 0x180D, 0x180E, 0x2000, 0x200F, 0x2028, 0x202F,
          0x205F, 0x2060, 0x206F, 0x2800, 0x3164, 0xFE00, 0xFE0F,
          0xFEFF, 0xFFA0, 0xFFF0, 0xFFFB, 0x1D176, 0xE0001, 0xE007F,
          0xE0100, 0xE01EF] as $point) {
    $inputs[] = 'Paper' . mb_chr($point, 'UTF-8') . 'Fold';
    if ($point > 0) { $inputs[] = 'Paper' . mb_chr($point - 1, 'UTF-8') . 'Fold'; }
    $inputs[] = 'Paper' . mb_chr($point + 1, 'UTF-8') . 'Fold';
}
$inputs = array_values(array_unique($inputs));
if (count($inputs) > 2048) { throw new RuntimeException('Fixture input count exceeds its bound.'); }
$cases = [];
foreach ($inputs as $input) {
    if (strlen($input) > 1024) { throw new RuntimeException('Fixture input exceeds its bound.'); }
    $cases[] = ['input' => $input, 'ascii' => normalize_ascii($input),
        'ascii_preserving_case' => normalize_ascii($input, 1),
        'text' => normalize_text($input), 'zero_width_removed' => strip_zerowidth($input)];
}
$json = json_encode(['reference' => 'operator-supplied 4chan-old checkout',
    'files' => ['lib/postfilter.php' => $hash], 'functions' => $names,
    'extractor_php' => PHP_VERSION, 'extractor_pcre' => PCRE_VERSION,
    'extractor_icu' => INTL_ICU_VERSION, 'extractor_icu_data' => INTL_ICU_DATA_VERSION,
    'cases' => $cases], JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE |
        JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (strlen($json) > 1048576) { throw new RuntimeException('Fixture output exceeds its bound.'); }
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Admission normalization reference differs.'); }
} elseif (file_put_contents($argv[2], $json) !== strlen($json)) {
    throw new RuntimeException('Admission normalization reference could not be saved.');
}
echo 'Admission normalization source cases: ' . count($cases) . ", ICU " . INTL_ICU_VERSION . "\n";
