<?php
// Execute the extracted pure table branch with synthetic extractor output only.
error_reporting(E_ALL & ~E_WARNING);
$source = json_decode(file_get_contents(__DIR__ . '/../fixtures/exif-comment-source.json'), true, 512, JSON_THROW_ON_ERROR);
define('ENABLE_EXIF', 1);
define('S_EXIF_TOGGLE', $source['toggle']);
eval('function source_exif($raw, $tim) { $exif = htmlspecialchars($raw); $exiftext = ""; ' . $source['table_php'] . ' return $exiftext; }');
$cases = [];
foreach ([1, 1730000000000, 9007199254740991] as $tim)
foreach (['', '0', 'Camera: Example', "Camera: Example\n", "Heading\nCamera: Example\n", 'Key: ', 'Key: 0', 'Key: first: ignored', ': value', ': ', "\n", "Camera: <script>\"&'", "A: x\r\nB: y", 'Key: [spoiler]x[/spoiler]', 'Key: >>123', 'Key: https://example.test', "Камера: 写真"] as $raw) {
    $cases[] = ['raw' => $raw, 'tim' => $tim, 'html' => source_exif($raw, $tim)];
}
$encoded = json_encode($cases, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
$path = __DIR__ . '/../crates/domain/tests/fixtures/exif-comment.json';
if (($argv[1] ?? '') === '--write') file_put_contents($path, $encoded);
elseif (file_get_contents($path) !== $encoded) throw new Exception('EXIF fixture differs');
echo 'Source EXIF table cases: ' . count($cases) . " passed\n";
