//! yt-dlp byte accounting across video/audio streams and fresh URL retries.
//! Counters describe retained media bytes, not network traffic or the last stream.
use serde_json::Value;
use std::collections::BTreeMap;

// Only size/codec/id metadata is printed: signed media URLs and HTTP headers
// must not become progress messages or normal task logs.
pub(crate) const PLAN_TEMPLATE: &str = "before_dl:[C2MD_PLAN] {\"formats\":%(requested_formats.:.{format_id,filesize,filesize_approx,vcodec,acodec})j,\"single\":%(.{format_id,filesize,filesize_approx,vcodec,acodec})j}";
pub(crate) const PROGRESS_TEMPLATE: &str = "download:[C2MD] %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(info.format_id)s %(info.vcodec)s %(info.acodec)s %(progress.status)s";

#[derive(Clone, Copy, Default)]
enum Kind {
    Video,
    Audio,
    #[default]
    Media,
}
impl Kind {
    fn from_codecs(video: &str, audio: &str) -> Self {
        match (video, audio) {
            ("none", audio) if !matches!(audio, "none" | "NA" | "") => Self::Audio,
            (video, "none") if !matches!(video, "none" | "NA" | "") => Self::Video,
            _ => Self::Media,
        }
    }
    fn label(self, cached: bool) -> &'static str {
        match (self, cached) {
            (Self::Video, false) => "下载视频 / Downloading video",
            (Self::Audio, false) => "下载音频 / Downloading audio",
            (Self::Media, false) => "下载视频和音频 / Downloading media",
            (Self::Video, true) => "复用已下载的视频 / Reusing downloaded video",
            (Self::Audio, true) => "复用已下载的音频 / Reusing downloaded audio",
            (Self::Media, true) => "复用已下载的媒体 / Reusing downloaded media",
        }
    }
}

#[derive(Default)]
struct Stream {
    current: u64,
    total: u64,
    estimated: bool,
    kind: Kind,
    finished: bool,
    sampled: bool,
}

pub(crate) struct Snapshot {
    pub current: u64,
    pub total: u64,
    pub message: String,
    pub reset_rate: bool,
    pub force: bool,
}

#[derive(Default)]
pub(crate) struct DownloadProgress {
    streams: BTreeMap<String, Stream>,
    last_format: String,
    last_message: String,
}
impl DownloadProgress {
    pub fn line(&mut self, line: &str) -> Option<Snapshot> {
        if let Some(plan) = line.strip_prefix("[C2MD_PLAN] ") {
            self.plan(plan);
            // Metadata/cached quantities are not a throughput sample.
            return None;
        }
        let sample = sample(line)?;
        let stream = self.streams.entry(sample.format.clone()).or_default();
        let cached = sample.finished && !stream.sampled;
        let reset_rate =
            cached || sample.current < stream.current || self.last_format != sample.format;
        stream.current = sample.current;
        if sample.total > 0 {
            stream.total = sample.total.max(sample.current);
            stream.estimated = sample.estimated;
        }
        stream.kind = sample.kind;
        stream.finished = sample.finished;
        stream.sampled = true;
        self.last_format = sample.format;
        let current = self
            .streams
            .values()
            .fold(0u64, |sum, s| sum.saturating_add(s.current));
        let known = self.streams.values().all(|s| s.total > 0);
        let total = if known {
            self.streams
                .values()
                .fold(0u64, |sum, s| sum.saturating_add(s.total))
        } else {
            0
        };
        let estimated = known && self.streams.values().any(|s| s.estimated);
        let finished = self.streams.values().all(|s| s.finished);
        let label = if finished {
            "整理下载文件 / Finalizing downloaded media"
        } else {
            sample.kind.label(cached)
        };
        let (zh, en) = label.split_once(" / ").expect("bilingual download label");
        let message = if estimated {
            format!("{zh} · 合计总量为估算 / {en}; estimated combined size")
        } else if !known {
            format!("{zh} · 合计总大小暂不可用 / {en}; combined size not yet available")
        } else {
            label.to_owned()
        };
        let force = reset_rate || finished || message != self.last_message;
        self.last_message = message.clone();
        Some(Snapshot {
            current,
            total,
            message,
            reset_rate,
            force,
        })
    }

    fn plan(&mut self, text: &str) {
        let Ok(plan) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let formats = plan["formats"].as_array().filter(|f| !f.is_empty());
        let formats: Vec<&Value> = match formats {
            Some(formats) => formats.iter().collect(),
            None => vec![&plan["single"]],
        };
        if formats.iter().any(|f| f["format_id"].as_str().is_none()) {
            return;
        }
        let mut previous = std::mem::take(&mut self.streams);
        for format in formats {
            let id = format["format_id"].as_str().unwrap().to_owned();
            let mut stream = previous.remove(&id).unwrap_or_default();
            if stream.total == 0 {
                let exact = json_bytes(&format["filesize"]).filter(|n| *n > 0);
                let estimated = json_bytes(&format["filesize_approx"]).filter(|n| *n > 0);
                stream.total = exact.or(estimated).unwrap_or(0);
                stream.estimated = exact.is_none() && estimated.is_some();
            }
            stream.kind = Kind::from_codecs(
                format["vcodec"].as_str().unwrap_or("NA"),
                format["acodec"].as_str().unwrap_or("NA"),
            );
            stream.sampled = false;
            self.streams.insert(id, stream);
        }
    }
}

struct Sample {
    current: u64,
    total: u64,
    estimated: bool,
    format: String,
    kind: Kind,
    finished: bool,
}
fn sample(line: &str) -> Option<Sample> {
    let mut fields = line.strip_prefix("[C2MD] ")?.split_whitespace();
    let current = fields.next().and_then(bytes);
    let exact = fields.next().and_then(bytes).filter(|n| *n > 0);
    let estimate = fields.next().and_then(bytes).filter(|n| *n > 0);
    let format = fields.next().unwrap_or("media").to_owned();
    let kind = Kind::from_codecs(fields.next().unwrap_or("NA"), fields.next().unwrap_or("NA"));
    let finished = fields.next() == Some("finished");
    Some(Sample {
        // yt-dlp's already-downloaded hook contains total_bytes but no downloaded_bytes.
        current: current.or_else(|| finished.then_some(exact).flatten())?,
        total: exact.or(estimate).unwrap_or(0),
        estimated: exact.is_none() && estimate.is_some(),
        format,
        kind,
        finished,
    })
}
fn bytes(text: &str) -> Option<u64> {
    text.parse::<u64>()
        .ok()
        .or_else(|| text.parse::<f64>().ok().and_then(float_bytes))
}
fn float_bytes(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0. && value < u64::MAX as f64).then(|| value.round() as u64)
}
fn json_bytes(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_f64().and_then(float_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn split_plan(progress: &mut DownloadProgress) {
        progress.line(r#"[C2MD_PLAN] {"formats":[{"format_id":"616","vcodec":"vp9","acodec":"none"},{"format_id":"140","vcodec":"none","acodec":"aac","filesize":40}],"single":{"format_id":"616+140"}}"#);
    }
    #[test]
    fn decimal_estimates_and_unknown_totals_are_parsed_without_inventing_sizes() {
        for (line, expected) in [
            ("[C2MD] 120326 10485760 NA", Some((120326, 10485760))),
            ("[C2MD] 120326 NA 20971520", Some((120326, 20971520))),
            ("[C2MD] 120326 NA 20971520.5", Some((120326, 20971521))),
            ("[C2MD] 120326 NA 2.097152e7", Some((120326, 20971520))),
            ("[C2MD] 120326 0 20971520.0", Some((120326, 20971520))),
            ("[C2MD] 120326 NA NA", Some((120326, 0))),
            ("[C2MD] 120326 NA NaN", Some((120326, 0))),
            ("[C2MD] 120326 NA inf", Some((120326, 0))),
            ("[C2MD] 120326 NA -1", Some((120326, 0))),
            ("[C2MD] NA NA NA", None),
            ("[download] 45.3% of 10MiB", None),
            ("", None),
        ] {
            assert_eq!(
                sample(line).map(|s| (s.current, s.total)),
                expected,
                "{line}"
            );
        }
    }
    #[test]
    fn video_and_audio_have_one_combined_counter_and_labeled_phases() {
        let mut p = DownloadProgress::default();
        split_plan(&mut p);
        let unknown = p.line("[C2MD] 1 NA NA 616 vp9 none downloading").unwrap();
        assert_eq!((unknown.current, unknown.total), (1, 0));
        let video = p
            .line("[C2MD] 50 NA 100.0 616 vp9 none downloading")
            .unwrap();
        assert_eq!((video.current, video.total), (50, 140));
        assert!(video.message.contains("下载视频") && video.message.contains("估算"));
        let video = p.line("[C2MD] 100 100 NA 616 vp9 none finished").unwrap();
        assert_eq!((video.current, video.total), (100, 140));
        let audio = p.line("[C2MD] 10 40 NA 140 none aac downloading").unwrap();
        assert_eq!((audio.current, audio.total), (110, 140));
        assert!(audio.message.contains("下载音频") && audio.reset_rate);
        let done = p.line("[C2MD] 40 40 NA 140 none aac finished").unwrap();
        assert_eq!((done.current, done.total), (140, 140));
        assert!(done.message.contains("整理下载文件"));
    }
    #[test]
    fn cached_streams_and_url_retries_do_not_double_count_bytes() {
        let mut p = DownloadProgress::default();
        split_plan(&mut p);
        p.line("[C2MD] 100 100 NA 616 vp9 none finished");
        p.line("[C2MD] 10 40 NA 140 none aac downloading");
        split_plan(&mut p); // New extraction, same retained files.
        let reused = p.line("[C2MD] NA 100 NA 616 vp9 none finished").unwrap();
        assert_eq!((reused.current, reused.total), (110, 140));
        assert!(reused.reset_rate && reused.message.contains("复用已下载的视频"));
        let audio = p.line("[C2MD] 20 40 NA 140 none aac downloading").unwrap();
        assert_eq!((audio.current, audio.total), (120, 140));
        let next = p.line("[C2MD] 30 40 NA 140 none aac downloading").unwrap();
        assert!(!next.reset_rate);
    }
    #[test]
    fn changed_formats_drop_obsolete_counters_and_single_media_also_works() {
        let mut p = DownloadProgress::default();
        split_plan(&mut p);
        p.line("[C2MD] 100 100 NA 616 vp9 none finished");
        p.line(r#"[C2MD_PLAN] {"formats":[],"single":{"format_id":"22","filesize":200,"vcodec":"avc1","acodec":"aac"}}"#);
        let media = p.line("[C2MD] 20 200 NA 22 avc1 aac downloading").unwrap();
        assert_eq!((media.current, media.total), (20, 200));
        assert!(media.message.contains("下载视频和音频"));
    }
}
