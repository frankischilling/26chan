//! Probe JSON is parsed only inside the disposable video guest.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Webm,
    Mp4,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metadata {
    pub container: Container,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f64,
    pub sample_aspect_ratio: Option<f64>,
    pub has_audio: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Unrecognized,
    Duration,
    AudioDisabled,
    AudioCodec,
    VideoCodec,
    Stream,
    Dimensions,
    MissingVideo,
}

impl Rejection {
    pub fn source_key(self) -> &'static str {
        match self {
            Self::Unrecognized => "S_NOREC",
            Self::Duration => "S_VIDEOTOOLONG",
            Self::AudioDisabled => "S_AUDIODISABLED",
            Self::AudioCodec => "S_BADAUDIO",
            Self::VideoCodec => "S_BADVIDEO",
            Self::Stream => "S_BADSTREAM",
            Self::Dimensions => "S_TOOLARGERES",
            Self::MissingVideo => "S_NOVIDEOSTREAM",
        }
    }
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

/// Match the ordered source decisions for well-formed probe records. Missing or
/// non-finite structure fails closed, never granting a format or board bypass.
pub fn validate(
    probe: &Value,
    maximum_duration: u32,
    maximum_dimension: u32,
    audio_enabled: bool,
) -> Result<Metadata, Rejection> {
    let container = match probe["format"]["format_name"].as_str() {
        Some("matroska,webm") => Container::Webm,
        Some("mov,mp4,m4a,3gp,3g2,mj2") => Container::Mp4,
        _ => return Err(Rejection::Unrecognized),
    };
    let duration_seconds = number(&probe["format"]["duration"]).ok_or(Rejection::Duration)?;
    if !duration_seconds.is_finite()
        || duration_seconds <= 0.0
        || duration_seconds > f64::from(maximum_duration)
    {
        return Err(Rejection::Duration);
    }
    let streams = probe["streams"].as_array().ok_or(Rejection::Stream)?;
    // The source remux selects streams before this stage. Bound malformed probe
    // data independently, even when the external program exited successfully.
    if streams.len() > 16 {
        return Err(Rejection::Stream);
    }
    let mut video = None;
    let mut has_audio = false;
    for stream in streams {
        let codec = stream["codec_name"].as_str().unwrap_or("");
        match stream["codec_type"].as_str() {
            Some("audio") => {
                if !audio_enabled {
                    return Err(Rejection::AudioDisabled);
                }
                if !matches!(
                    (container, codec),
                    (Container::Webm, "opus" | "vorbis") | (Container::Mp4, "aac")
                ) {
                    return Err(Rejection::AudioCodec);
                }
                has_audio = true;
            }
            Some("video") => {
                if video.is_some() {
                    return Err(Rejection::Stream);
                }
                if !matches!(
                    (container, codec),
                    (Container::Webm, "vp8" | "vp9") | (Container::Mp4, "h264")
                ) {
                    return Err(Rejection::VideoCodec);
                }
                if container == Container::Mp4 {
                    // ffprobe can omit bits_per_raw_sample for ordinary 8-bit
                    // H.264. The source permits that case; pix_fmt still binds it.
                    let bits = if stream["bits_per_raw_sample"].is_null() {
                        0.0
                    } else {
                        number(&stream["bits_per_raw_sample"]).ok_or(Rejection::Unrecognized)?
                    };
                    if !bits.is_finite()
                        || !(0.0..=8.0).contains(&bits)
                        || stream["pix_fmt"] != "yuv420p"
                    {
                        return Err(Rejection::Unrecognized);
                    }
                }
                let width = stream["width"].as_u64().ok_or(Rejection::Dimensions)?;
                let height = stream["height"].as_u64().ok_or(Rejection::Dimensions)?;
                if width == 0
                    || height == 0
                    || width > u64::from(maximum_dimension)
                    || height > u64::from(maximum_dimension)
                {
                    return Err(Rejection::Dimensions);
                }
                let sar = stream["sample_aspect_ratio"].as_str().and_then(|value| {
                    let (numerator, denominator) = value.split_once(':')?;
                    let numerator = numerator.parse::<i32>().ok()?;
                    let denominator = denominator.parse::<i32>().ok()?;
                    if denominator == 0 || numerator == denominator {
                        return None;
                    }
                    let ratio = f64::from(numerator) / f64::from(denominator);
                    (ratio > 0.5 && ratio < 2.0).then_some(ratio)
                });
                video = Some((width as u32, height as u32, sar));
            }
            _ => return Err(Rejection::Stream),
        }
    }
    let (width, height, sample_aspect_ratio) = video.ok_or(Rejection::MissingVideo)?;
    Ok(Metadata {
        container,
        width,
        height,
        duration_seconds,
        sample_aspect_ratio,
        has_audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_source_decisions() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../fixtures/source-video-reference.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let actual = validate(
                &case["probe"],
                case["max_duration"].as_u64().unwrap() as u32,
                case["max_dimension"].as_u64().unwrap() as u32,
                case["audio_enabled"].as_bool().unwrap(),
            );
            if let Some(key) = case["expected"]["error"].as_str() {
                assert_eq!(actual.unwrap_err().source_key(), key, "{}", case["id"]);
            } else {
                let actual = actual.unwrap_or_else(|error| panic!("{}: {error:?}", case["id"]));
                let expected = &case["expected"]["dimensions"];
                assert_eq!(u64::from(actual.width), expected[0].as_u64().unwrap());
                assert_eq!(u64::from(actual.height), expected[1].as_u64().unwrap());
                assert_eq!(
                    actual.sample_aspect_ratio,
                    expected[2].as_f64(),
                    "{}",
                    case["id"]
                );
            }
        }
    }

    #[test]
    fn malformed_probe_data_is_not_authority() {
        let base = serde_json::json!({"format": {"format_name": "matroska,webm", "duration": "1"},
            "streams": [{"codec_type": "video", "codec_name": "vp8", "width": 640, "height": 480}]});
        for duration in ["NaN", "inf", "-inf", "1e309", "-0"] {
            let mut probe = base.clone();
            probe["format"]["duration"] = duration.into();
            assert_eq!(validate(&probe, 400, 2048, true), Err(Rejection::Duration));
        }
        for dimensions in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!(null),
        ] {
            let mut probe = base.clone();
            probe["streams"][0]["width"] = dimensions;
            assert_eq!(
                validate(&probe, 400, 2048, true),
                Err(Rejection::Dimensions)
            );
        }
    }
}
