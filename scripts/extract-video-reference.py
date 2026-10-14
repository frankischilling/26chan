"""Independent video admission vectors from the pinned source probe consumer.

The optional PHP oracle executes only the captured decision tail, with synthetic
probe records. It never starts FFmpeg, reads uploads, or boots the application.
Actual remux/decode and browser qualification are separate requirements.
"""
import argparse
import copy
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'fixtures/source-video-reference.json'
SOURCE_SHA = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
TAIL_SHA = '10aefcdce056746af7b1afb739193ecab7bd1f4f781c63e4191006143c121cb2'
TAIL_START = b"  if ($res['format']['format_name'] === 'matroska,webm')"
TAIL_END = b'  return $video_dims;'


def cases():
    result = []
    base = {'format': {'format_name': 'matroska,webm', 'duration': '1.25'},
            'streams': [{'codec_type': 'video', 'codec_name': 'vp8',
                         'width': 640, 'height': 480, 'sample_aspect_ratio': '1:1'}]}

    def add(name, record=None, expected=None, *, audio=False, duration=120):
        result.append({'id': name, 'probe': copy.deepcopy(record or base),
                       'audio_enabled': audio, 'max_duration': duration,
                       'max_dimension': 2048,
                       'expected': expected or {'dimensions': [640, 480, None]}})

    add('vp8')
    for codec in ['vp9', 'av1', 'h264', 'theora', '']:
        record = copy.deepcopy(base)
        record['streams'][0]['codec_name'] = codec
        add('webm-codec-' + (codec or 'empty'), record,
            None if codec == 'vp9' else {'error': 'S_BADVIDEO'})
    for value, accepted in [('0', False), ('-1', False), ('0.001', True),
                            ('119.999', True), ('120', True), ('120.001', False),
                            ('N/A', False), ('', False)]:
        record = copy.deepcopy(base)
        record['format']['duration'] = value
        add('duration-' + (value or 'empty'), record,
            None if accepted else {'error': 'S_VIDEOTOOLONG'})
    for maximum in [300, 400]:
        for value in [maximum, maximum + 0.001]:
            record = copy.deepcopy(base)
            record['format']['duration'] = str(value)
            add(f'board-duration-{maximum}-{value}', record,
                None if value == maximum else {'error': 'S_VIDEOTOOLONG'}, duration=maximum)
    for field in ['width', 'height']:
        for value in [0, 1, 2048, 2049]:
            record = copy.deepcopy(base)
            record['streams'][0][field] = value
            dimensions = [record['streams'][0]['width'], record['streams'][0]['height'], None]
            add(f'{field}-{value}', record,
                {'dimensions': dimensions} if 1 <= value <= 2048 else {'error': 'S_TOOLARGERES'})
    for sar, expected in [('0:1', None), ('1:0', None), ('1:2', None),
                          ('2:1', None), ('3:2', 1.5), ('2:3', 2/3), ('1:1', None)]:
        record = copy.deepcopy(base)
        record['streams'][0]['sample_aspect_ratio'] = sar
        add('sar-' + sar, record, {'dimensions': [640, 480, expected]})
    record = copy.deepcopy(base)
    del record['streams'][0]['sample_aspect_ratio']
    add('sar-absent', record)
    for codec in ['vorbis', 'opus', 'aac', 'mp3']:
        for enabled in [False, True]:
            record = copy.deepcopy(base)
            record['streams'].append({'codec_type': 'audio', 'codec_name': codec})
            error = 'S_AUDIODISABLED' if not enabled else None if codec in ['vorbis', 'opus'] else 'S_BADAUDIO'
            add(f'webm-audio-{codec}-{enabled}', record,
                {'error': error} if error else None, audio=enabled)
    mp4 = copy.deepcopy(base)
    mp4['format']['format_name'] = 'mov,mp4,m4a,3gp,3g2,mj2'
    mp4['streams'][0].update(codec_name='h264', bits_per_raw_sample='8', pix_fmt='yuv420p')
    add('mp4-h264', mp4)
    for field, value, error in [('codec_name', 'hevc', 'S_BADVIDEO'),
                               ('codec_name', 'vp9', 'S_BADVIDEO'),
                               ('bits_per_raw_sample', '10', 'S_NOREC'),
                               ('pix_fmt', 'yuv444p', 'S_NOREC'),
                               ('pix_fmt', 'yuv420p10le', 'S_NOREC')]:
        record = copy.deepcopy(mp4)
        record['streams'][0][field] = value
        add(f'mp4-{field}-{value}', record, {'error': error})
    for codec in ['aac', 'opus', 'vorbis']:
        for enabled in [False, True]:
            record = copy.deepcopy(mp4)
            record['streams'].append({'codec_type': 'audio', 'codec_name': codec})
            error = 'S_AUDIODISABLED' if not enabled else None if codec == 'aac' else 'S_BADAUDIO'
            add(f'mp4-audio-{codec}-{enabled}', record,
                {'error': error} if error else None, audio=enabled)
    for kind in ['subtitle', 'data', 'attachment']:
        record = copy.deepcopy(base)
        record['streams'].append({'codec_type': kind})
        add('extra-stream-' + kind, record, {'error': 'S_BADSTREAM'})
    record = copy.deepcopy(base)
    record['streams'] *= 2
    add('multiple-video', record, {'error': 'S_BADSTREAM'})
    record = copy.deepcopy(base)
    record['streams'] = []
    add('no-stream', record, {'error': 'S_NOVIDEOSTREAM'})
    record['streams'] = [{'codec_type': 'audio', 'codec_name': 'opus'}]
    add('audio-only', record, {'error': 'S_NOVIDEOSTREAM'}, audio=True)
    record = copy.deepcopy(base)
    record['streams'] += [{'codec_type': 'audio', 'codec_name': 'opus'}] * 2
    add('multiple-valid-audio', record, audio=True)
    record = copy.deepcopy(base)
    record['format']['format_name'] = 'avi'
    record['format']['duration'] = '0'
    add('container-before-duration', record, {'error': 'S_NOREC'})
    record = copy.deepcopy(base)
    record['format']['duration'] = '0'
    record['streams'] = []
    add('duration-before-streams', record, {'error': 'S_VIDEOTOOLONG'})
    for audio_first in [False, True]:
        record = copy.deepcopy(base)
        record['streams'][0]['codec_name'] = 'av1'
        record['streams'].append({'codec_type': 'audio', 'codec_name': 'aac'})
        if audio_first:
            record['streams'].reverse()
        add(f'stream-order-{audio_first}', record,
            {'error': 'S_AUDIODISABLED' if audio_first else 'S_BADVIDEO'})
    return result


def php_oracle(fixture, php):
    # Literal substitutions supply configuration, not alternate admission code.
    tail = fixture['decision_tail']
    for constant, variable in [('MAX_WEBM_DURATION', '$maximumDuration'),
                               ('MAX_WEBM_DIMENSION', '$maximumDimension'),
                               ('ENABLE_WEBM_AUDIO', '$audioEnabled')]:
        tail = tail.replace(constant, variable)
    errors = sorted({case['expected']['error'] for case in fixture['cases'] if 'error' in case['expected']})
    program = '<?php\n' + '\n'.join(f"define('{name}', '{name}');" for name in errors)
    program += '\nfunction error($message, $file) { throw new RuntimeException($message); }\n'
    program += 'function decision($res, $maximumDuration, $maximumDimension, $audioEnabled) { $file = null;\n' + tail + '\n}\n'
    program += '''$cases = json_decode(stream_get_contents(STDIN), true, 512, JSON_THROW_ON_ERROR);
$results = [];
foreach ($cases as $case) {
  try {
    $result = ['dimensions' => decision($case['probe'], $case['max_duration'], $case['max_dimension'], $case['audio_enabled'])];
  } catch (RuntimeException $error) { $result = ['error' => $error->getMessage()]; }
  $results[] = $result;
}
echo json_encode($results, JSON_THROW_ON_ERROR);
'''
    # PHP -r code does not use the opening tag. No source application is loaded.
    completed = subprocess.run([php, '-n', '-d', 'allow_url_fopen=0', '-d', 'allow_url_include=0',
                                '-d', 'disable_functions=exec,shell_exec,system,passthru,proc_open,popen',
                                '-r', program.removeprefix('<?php\n')],
                               input=json.dumps(fixture['cases']), capture_output=True, text=True,
                               timeout=10, check=True)
    if completed.stderr:
        raise ValueError('PHP oracle emitted diagnostics: ' + completed.stderr)
    actual = json.loads(completed.stdout)
    expected = [case['expected'] for case in fixture['cases']]
    if actual != expected:
        differences = [(case['id'], want, got) for case, want, got in zip(fixture['cases'], expected, actual) if want != got]
        raise ValueError(f'PHP source differs: {differences}; result count {len(actual)}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--write', action='store_true')
    parser.add_argument('--php-oracle', action='store_true')
    parser.add_argument('--php', default='php')
    args = parser.parse_args()
    if args.source:
        source = (args.source / 'imgboard.php').read_bytes()
        if hashlib.sha256(source).hexdigest() != SOURCE_SHA:
            raise ValueError('Source file differs from the reviewed revision')
        start = source.index(TAIL_START)
        tail = source[start:source.index(TAIL_END, start) + len(TAIL_END)].decode()
    else:
        tail = json.loads(FIXTURE.read_text())['decision_tail']
    # Pin the extracted decision bytes independently of the fixture itself.
    if hashlib.sha256(tail.encode()).hexdigest() != TAIL_SHA:
        raise ValueError('Source decision excerpt differs')
    fixture = {'schema_version': 1, 'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
               'source_sha256': SOURCE_SHA, 'decision_tail': tail, 'cases': cases()}
    if args.write:
        if not args.source:
            raise ValueError('Writing requires the actual reference checkout')
        FIXTURE.write_text(json.dumps(fixture, indent=2) + '\n')
    elif json.loads(FIXTURE.read_text()) != fixture:
        raise ValueError('Video fixture differs from the independent expectations')
    if args.php_oracle:
        php_oracle(fixture, args.php)
    print(f'Video source reference: {len(fixture["cases"])} cases; PHP oracle: {args.php_oracle}')


if __name__ == '__main__':
    main()
