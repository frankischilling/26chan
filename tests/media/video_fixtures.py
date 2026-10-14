"""Generate tiny owned color/tone clips; never reads a user upload."""
import pathlib
import subprocess


def generate(root):
    root = pathlib.Path(root)
    root.mkdir(exist_ok=True)
    cases = [('red.webm', 'red', 'libvpx', None),
             ('blue.mp4', 'blue', 'libx264', None),
             ('red-opus.webm', 'red', 'libvpx', 'libopus'),
             ('blue-aac.mp4', 'blue', 'libx264', 'aac'),
             ('red-vp9.webm', 'red', 'libvpx-vp9', 'libvorbis')]
    for name, color, video, audio in cases:
        command = ['/usr/bin/ffmpeg', '-nostdin', '-v', 'error', '-f', 'lavfi',
                   '-i', f'color=c={color}:s=64x48:r=10']
        if audio:
            command += ['-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000', '-c:a', audio]
        command += ['-t', '0.5', '-c:v', video, '-threads', '1', '-pix_fmt', 'yuv420p', '-n', str(root / name)]
        subprocess.run(command, check=True, capture_output=True, timeout=15)

    subprocess.run(['/usr/bin/ffmpeg', '-nostdin', '-v', 'error', '-display_rotation', '90',
                    '-i', str(root / 'blue.mp4'), '-c', 'copy', '-n', str(root / 'blue-rotation90.mp4')],
                   check=True, capture_output=True, timeout=15)
    subprocess.run(['/usr/bin/ffmpeg', '-nostdin', '-v', 'error', '-i', str(root / 'blue.mp4'),
                    '-c', 'copy', '-aspect', '2:1', '-n', str(root / 'blue-sar.mp4')],
                   check=True, capture_output=True, timeout=15)


if __name__ == '__main__':
    import sys
    if len(sys.argv) != 2:
        raise SystemExit('usage: video_fixtures.py NEW_CLIP_DIRECTORY')
    generate(sys.argv[1])
