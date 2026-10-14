//! Fixed-process video pipeline for the disposable guest. No user argument is
//! interpreted as a pathname, option, protocol, or executable.
use crate::video_probe::{self, Container, Metadata};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIAGNOSTICS: usize = 64 * 1024;
pub const OUTPUT_BYTES: usize = 9 * 1024 * 1024;
const THUMBNAIL: u32 = 250;

fn rejected() -> io::Error {
    io::Error::other("video processing rejected")
}

fn format(container: Container) -> &'static str {
    match container {
        Container::Webm => "webm",
        Container::Mp4 => "mp4",
    }
}

fn run(
    program: &Path,
    arguments: &[&str],
    maximum: usize,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    if Instant::now() >= deadline {
        return Err(rejected());
    }
    let mut child = Command::new(program)
        .args(arguments)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(rejected)?;
    let stderr = child.stderr.take().ok_or_else(rejected)?;
    thread::scope(|scope| {
        let out = scope.spawn(move || bounded_read(stdout, maximum));
        let err = scope.spawn(move || bounded_read(stderr, MAX_DIAGNOSTICS));
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(rejected());
                }
            }
        };
        let output = out.join().map_err(|_| rejected())??;
        let errors = err.join().map_err(|_| rejected())??;
        if !status?.success()
            || errors
                .windows(7)
                .any(|word| word.eq_ignore_ascii_case(b"invalid"))
        {
            return Err(rejected());
        }
        Ok(output)
    })
}

fn bounded_read(input: impl Read, maximum: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(rejected());
    }
    Ok(bytes)
}

struct Files {
    input: PathBuf,
    remux: PathBuf,
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.input);
        let _ = fs::remove_file(&self.remux);
    }
}

fn thumbnail(metadata: &Metadata) -> (u32, u32) {
    let mut width = f64::from(metadata.width);
    let mut height = f64::from(metadata.height);
    if let Some(ratio) = metadata.sample_aspect_ratio {
        if ratio > 1.0 {
            height = (height / ratio).round().max(1.0);
        } else {
            width = (width * ratio).round().max(1.0);
        }
    }
    let scale = (f64::from(THUMBNAIL) / width.max(height)).min(1.0);
    (
        (width * scale).floor() as u32,
        (height * scale).floor() as u32,
    )
}

fn process(
    input: &[u8],
    container: Container,
    ffmpeg: &Path,
    ffprobe: &Path,
    root: &Path,
) -> io::Result<Vec<u8>> {
    if input.is_empty() || input.len() > MAX_BYTES {
        return Err(rejected());
    }
    let deadline = Instant::now() + Duration::from_secs(6);
    let input_path = root.join("video-input");
    let remux_path = root.join("video-remux");
    if remux_path.try_exists()? {
        return Err(rejected());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&input_path)?;
    let files = Files {
        input: input_path,
        remux: remux_path,
    };
    file.write_all(input)?;
    drop(file);
    let input_path = files.input.to_str().ok_or_else(rejected)?;
    let remux_path = files.remux.to_str().ok_or_else(rejected)?;
    let kind = format(container);
    run(
        ffmpeg,
        &[
            "-nostdin",
            "-v",
            "error",
            "-threads",
            "1",
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            kind,
            "-i",
            input_path,
            "-map_metadata",
            "-1",
            "-bitexact",
            "-c",
            "copy",
            "-f",
            kind,
            "-n",
            remux_path,
        ],
        0,
        deadline,
    )?;
    let remux = bounded_read(fs::File::open(&files.remux)?, MAX_BYTES)?;
    if remux.is_empty() {
        return Err(rejected());
    }
    let probe = run(
        ffprobe,
        &[
            "-v",
            "error",
            "-threads",
            "1",
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            kind,
            "-i",
            remux_path,
            "-show_entries",
            "format=format_name,duration:stream=codec_type,codec_name,width,height,sample_aspect_ratio,bits_per_raw_sample,pix_fmt",
            "-of",
            "json",
        ],
        MAX_DIAGNOSTICS,
        deadline,
    )?;
    let probe = serde_json::from_slice(&probe).map_err(|_| rejected())?;
    // Board-specific rules are rechecked by the posting transaction. This guest
    // ceiling covers the largest imported duration and allows either audio policy.
    let metadata = video_probe::validate(&probe, 400, 2048, true).map_err(|_| rejected())?;
    if metadata.container != container {
        return Err(rejected());
    }
    let (width, height) = thumbnail(&metadata);
    if width == 0 || height == 0 {
        return Err(rejected());
    }
    let filter = format!("scale={width}:{height}:flags=bicubic,format=rgba");
    let preview_bytes = width as usize * height as usize * 4;
    let preview = run(
        ffmpeg,
        &[
            "-nostdin",
            "-v",
            "error",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            kind,
            "-i",
            remux_path,
            "-frames:v",
            "1",
            "-an",
            "-vf",
            &filter,
            "-threads",
            "1",
            "-f",
            "rawvideo",
            "pipe:1",
        ],
        preview_bytes,
        deadline,
    )?;
    if preview.len() != preview_bytes {
        return Err(rejected());
    }
    let mut output = Vec::with_capacity(64 + remux.len() + preview.len());
    output.extend_from_slice(b"IBVID005");
    output.push(match container {
        Container::Webm => 1,
        Container::Mp4 => 2,
    });
    output.push(u8::from(metadata.has_audio));
    output.extend_from_slice(&[0; 6]);
    output.extend_from_slice(&metadata.width.to_be_bytes());
    output.extend_from_slice(&metadata.height.to_be_bytes());
    output.extend_from_slice(
        &((metadata.duration_seconds * 1_000_000.0).ceil() as u64).to_be_bytes(),
    );
    output.extend_from_slice(&metadata.sample_aspect_ratio.unwrap_or(0.0).to_be_bytes());
    output.extend_from_slice(&width.to_be_bytes());
    output.extend_from_slice(&height.to_be_bytes());
    output.extend_from_slice(&(remux.len() as u64).to_be_bytes());
    output.extend_from_slice(&[0; 8]);
    output.extend_from_slice(&remux);
    output.extend_from_slice(&preview);
    Ok(output)
}

/// A single explicit video job. The VM is stopped before its disk is consumed.
pub fn decode(input: &[u8], container: Container) -> io::Result<Vec<u8>> {
    process(
        input,
        container,
        Path::new("/usr/bin/board-ffmpeg"),
        Path::new("/usr/bin/board-ffprobe"),
        Path::new("/tmp"),
    )
}

pub fn decode_disk(mut input: impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; 9];
    input.read_exact(&mut header)?;
    let length = u64::from_be_bytes(header[..8].try_into().map_err(|_| rejected())?);
    if !(2..=MAX_BYTES as u64 + 1).contains(&length) {
        return Err(rejected());
    }
    let container = match header[8] {
        1 => Container::Webm,
        2 => Container::Mp4,
        _ => return Err(rejected()),
    };
    let mut bytes = vec![0; length as usize - 1];
    input.read_exact(&mut bytes)?;
    let padding_size = (512 - (8 + length as usize) % 512) % 512;
    let mut padding = [0; 511];
    input.read_exact(&mut padding[..padding_size])?;
    if padding[..padding_size].iter().any(|&byte| byte != 0) || input.read(&mut [0; 1])? != 0 {
        return Err(rejected());
    }
    decode(&bytes, container)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_respect_source_sample_aspect_and_preview_cap() {
        let mut metadata = Metadata {
            container: Container::Webm,
            width: 640,
            height: 480,
            duration_seconds: 1.0,
            sample_aspect_ratio: None,
            has_audio: false,
        };
        assert_eq!(thumbnail(&metadata), (250, 187));
        metadata.sample_aspect_ratio = Some(1.5);
        assert_eq!(thumbnail(&metadata), (250, 125));
        metadata.sample_aspect_ratio = Some(2.0 / 3.0);
        assert_eq!(thumbnail(&metadata), (222, 250));
        metadata.width = 1;
        metadata.height = 2048;
        metadata.sample_aspect_ratio = None;
        assert_eq!(thumbnail(&metadata), (0, 250));
    }

    #[test]
    fn invalid_framing_never_starts_a_processor() {
        for bytes in [vec![], vec![0; 9], vec![255; 9]] {
            assert!(decode_disk(bytes.as_slice()).is_err());
        }
    }

    #[test]
    #[ignore = "requires explicitly prepared owned clips and the reviewed minimal runtime"]
    fn owned_runtime_remuxes_and_decodes_both_containers() {
        let tools = PathBuf::from(std::env::var_os("VIDEO_TEST_RUNTIME").expect("owned runtime"));
        let owned = PathBuf::from(std::env::var_os("VIDEO_TEST_CLIPS").expect("owned clips"));
        let root = owned.join(format!("scratch-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        for (name, container, format_byte, channel, audio) in [
            ("red.webm", Container::Webm, 1, 0, 0),
            ("blue.mp4", Container::Mp4, 2, 2, 0),
            ("red-opus.webm", Container::Webm, 1, 0, 1),
            ("blue-aac.mp4", Container::Mp4, 2, 2, 1),
        ] {
            let bytes = fs::read(owned.join(name)).unwrap();
            let output = process(
                &bytes,
                container,
                &tools.join("ffmpeg"),
                &tools.join("ffprobe"),
                &root,
            )
            .unwrap();
            assert_eq!(&output[..8], b"IBVID005");
            assert_eq!(output[8], format_byte);
            assert_eq!(output[9], audio);
            assert_eq!(u32::from_be_bytes(output[16..20].try_into().unwrap()), 64);
            assert_eq!(u32::from_be_bytes(output[20..24].try_into().unwrap()), 48);
            let length = u64::from_be_bytes(output[48..56].try_into().unwrap()) as usize;
            assert!(length > 0 && length <= MAX_BYTES);
            assert_eq!(output.len(), 64 + length + 64 * 48 * 4);
            for pixel in output[64 + length..].chunks_exact(4) {
                assert!(pixel[channel] >= 240, "{name}: {pixel:?}");
                assert_eq!(pixel[3], 255);
            }
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        }
        assert!(
            process(
                b"not a video",
                Container::Webm,
                &tools.join("ffmpeg"),
                &tools.join("ffprobe"),
                &root
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(root).unwrap();
    }
}
