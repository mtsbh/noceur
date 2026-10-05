//! Converting between lossless formats, through ffmpeg.
//!
//! WAV, AIFF, FLAC and ALAC hold exactly the same thing - integer PCM at some
//! depth and rate - so converting between them loses nothing, provided nothing
//! on the way changes the samples. That is the whole job here: pick the codec
//! that stores the source's own depth, keep its rate and channels, and prove it
//! afterwards by decoding both files and comparing the audio bit for bit.
//!
//! ffmpeg does the encoding rather than a crate per format because it is the
//! one tool that writes all four correctly, and it is found rather than bundled:
//! beside the executable, at a path set in settings, or on `PATH`.
//!
//! Tags and art are copied by lofty afterwards rather than by ffmpeg, which
//! writes almost nothing into a WAV or an AIFF. lofty puts an ID3v2 tag in both,
//! which is what Rekordbox, Serato and Traktor read there.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::config::Config;
use crate::state::AppState;

/// A format to convert into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Wav,
    Aiff,
    Flac,
    Alac,
}

impl Format {
    fn ext(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Aiff => "aiff",
            Format::Flac => "flac",
            Format::Alac => "m4a",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Format::Wav => "WAV",
            Format::Aiff => "AIFF",
            Format::Flac => "FLAC",
            Format::Alac => "ALAC",
        }
    }

    /// The muxer, named rather than guessed from the temporary file's name.
    fn muxer(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Aiff => "aiff",
            Format::Flac => "flac",
            Format::Alac => "ipod",
        }
    }
}

/// How samples are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    Int(u32),
    Float(u32),
}

impl Depth {
    fn label(self) -> String {
        match self {
            Depth::Int(b) => format!("{b}-bit"),
            Depth::Float(b) => format!("{b}-bit float"),
        }
    }
}

/// What ffmpeg says a file's first audio stream is.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub codec: String,
    pub rate: u32,
    pub depth: Depth,
    pub lossless: bool,
}

impl Stream {
    fn label(&self) -> String {
        let codec = if self.codec.starts_with("pcm_") {
            "PCM".to_string()
        } else {
            self.codec.to_uppercase()
        };
        let khz = self.rate as f64 / 1000.0;
        if self.lossless {
            format!("{codec} {} {khz} kHz", self.depth.label())
        } else {
            format!("{codec} {khz} kHz")
        }
    }
}

const LOSSLESS: &[&str] = &[
    "flac", "alac", "wavpack", "ape", "tta", "tak", "mlp", "truehd", "shorten", "mp4als",
];

/// Reads the stream line out of `ffmpeg -i`'s report.
///
/// It looks like `Stream #0:0: Audio: flac, 44100 Hz, stereo, s32 (24 bit)`.
/// The sample format says how the decoder holds the samples, and the bracketed
/// width - when there is one - how many of those bits are real: a 24-bit file
/// decodes into 32-bit words.
pub fn parse_stream(report: &str) -> Option<Stream> {
    let line = report.lines().find(|l| l.contains("Audio: "))?;
    let rest = &line[line.find("Audio: ")? + 7..];
    let parts: Vec<&str> = rest.split(", ").collect();
    let codec = parts.first()?.split_whitespace().next()?.to_string();

    let rate = parts
        .iter()
        .find_map(|p| p.strip_suffix(" Hz").and_then(|n| n.trim().parse().ok()))
        .unwrap_or(0);

    let mut depth = None;
    for p in &parts {
        let mut words = p.split_whitespace();
        let Some(fmt) = words.next() else { continue };
        let base = match fmt {
            "u8" | "u8p" | "s16" | "s16p" => Some(Depth::Int(16)),
            "s32" | "s32p" | "s64" | "s64p" => Some(Depth::Int(32)),
            "flt" | "fltp" => Some(Depth::Float(32)),
            "dbl" | "dblp" => Some(Depth::Float(64)),
            _ => None,
        };
        if let Some(base) = base {
            let real = p
                .find('(')
                .and_then(|i| p[i + 1..].split_whitespace().next())
                .and_then(|n| n.parse::<u32>().ok());
            depth = Some(match (base, real) {
                (Depth::Int(_), Some(bits)) => Depth::Int(bits.clamp(8, 32).max(16)),
                (d, _) => d,
            });
            break;
        }
    }

    let lossless = codec.starts_with("pcm_") || LOSSLESS.contains(&codec.as_str());
    Some(Stream {
        codec,
        rate,
        depth: depth.unwrap_or(Depth::Float(32)),
        lossless,
    })
}

/// The depth asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Want {
    /// Whatever the source has, which is what makes a conversion lossless.
    Source,
    #[serde(rename = "16")]
    Sixteen,
    #[serde(rename = "24")]
    TwentyFour,
}

/// How one file will be encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub depth: Depth,
    /// Codec arguments for ffmpeg.
    pub args: Vec<String>,
    /// Whether the output holds exactly the source's samples - and so can be
    /// checked against it.
    pub exact: bool,
    /// Why this cannot be done, when it cannot.
    pub problem: Option<String>,
}

/// Picks the codec that stores the source unchanged, or as close as asked.
pub fn recipe(src: &Stream, format: Format, want: Want) -> Recipe {
    let target = match want {
        Want::Sixteen => Depth::Int(16),
        Want::TwentyFour => Depth::Int(24),
        Want::Source if !src.lossless => Depth::Int(24),
        Want::Source => match (src.depth, format) {
            // FLAC and ALAC hold integers only.
            (Depth::Float(_), Format::Flac | Format::Alac) => Depth::Int(24),
            (d, _) => d,
        },
    };

    let mut problem = None;
    if format == Format::Alac && target == Depth::Int(32) {
        problem = Some("ALAC holds up to 24-bit".to_string());
    }

    let s = |v: &str| v.to_string();
    let mut args: Vec<String> = match (format, target) {
        (Format::Wav, Depth::Int(b)) => vec![s("-c:a"), format!("pcm_s{}le", b.max(16))],
        (Format::Wav, Depth::Float(b)) => vec![s("-c:a"), format!("pcm_f{b}le")],
        (Format::Aiff, Depth::Int(b)) => vec![s("-c:a"), format!("pcm_s{}be", b.max(16))],
        (Format::Aiff, Depth::Float(b)) => vec![s("-c:a"), format!("pcm_f{b}be")],
        (Format::Flac, Depth::Int(16)) => {
            vec![s("-c:a"), s("flac"), s("-sample_fmt"), s("s16")]
        }
        (Format::Flac, Depth::Int(b)) => {
            let mut a = vec![
                s("-c:a"),
                s("flac"),
                s("-sample_fmt"),
                s("s32"),
                s("-bits_per_raw_sample"),
                b.to_string(),
            ];
            if b > 24 {
                // 32-bit FLAC is newer than most decoders; ffmpeg asks for it
                // to be meant.
                a.extend([s("-strict"), s("experimental")]);
            }
            a
        }
        (Format::Alac, Depth::Int(16)) => {
            vec![s("-c:a"), s("alac"), s("-sample_fmt"), s("s16p")]
        }
        (Format::Alac, Depth::Int(b)) => vec![
            s("-c:a"),
            s("alac"),
            s("-sample_fmt"),
            s("s32p"),
            s("-bits_per_raw_sample"),
            b.to_string(),
        ],
        (Format::Flac | Format::Alac, Depth::Float(_)) => unreachable!("mapped to 24-bit above"),
    };
    if format == Format::Flac {
        args.extend([s("-compression_level"), s("8")]);
    }
    if format == Format::Wav {
        // A long 32-bit or multichannel file passes 4 GB, past which a plain
        // WAV header cannot say how long it is.
        args.extend([s("-rf64"), s("auto")]);
    }

    // Fewer bits than the source: dither, so the lost bits turn into a hiss
    // below hearing rather than distortion that follows the music.
    let narrower = match (src.depth, target) {
        (Depth::Int(a), Depth::Int(b)) => b < a,
        (Depth::Float(_), Depth::Int(_)) => true,
        _ => false,
    };
    if narrower && src.lossless {
        args.extend([s("-af"), s("aresample=dither_method=triangular")]);
    }

    Recipe {
        depth: target,
        exact: src.lossless && src.depth == target,
        args,
        problem,
    }
}

/// Where ffmpeg is, looking where a portable app would keep it first.
pub fn find_ffmpeg(configured: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };

    let configured = configured.trim().trim_matches('"');
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        // A folder is as good as the file in it.
        let p = if p.is_dir() { p.join(exe) } else { p };
        if p.is_file() {
            return Some(p);
        }
    }

    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        for rel in [
            exe.to_string(),
            format!("ffmpeg/{exe}"),
            format!("ffmpeg/bin/{exe}"),
        ] {
            let p = dir.join(rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(exe))
            .find(|p| p.is_file())
    })
}

/// A command for ffmpeg that does not flash a console window on Windows.
fn ffmpeg(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    cmd.arg("-hide_banner").arg("-nostdin");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn probe(ff: &Path, file: &Path) -> Result<Stream, String> {
    let out = ffmpeg(ff)
        .arg("-i")
        .arg(file)
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    // Exits with an error because no output was named; the report is on
    // stderr either way.
    let report = String::from_utf8_lossy(&out.stderr);
    parse_stream(&report).ok_or_else(|| "no audio stream".to_string())
}

/// A digest of the decoded audio, for comparing two files sample by sample.
///
/// Decoded to 32-bit integers (or 64-bit floats for float sources), which
/// every narrower integer widens into exactly - so a 24-bit FLAC and the
/// 24-bit WAV made from it produce the same digest, and any changed sample
/// changes it.
fn digest(ff: &Path, file: &Path, float: bool) -> Result<String, String> {
    let codec = if float { "pcm_f64le" } else { "pcm_s32le" };
    let out = ffmpeg(ff)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-map", "0:a:0", "-c:a", codec, "-f", "md5", "-"])
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .strip_prefix("MD5=")
        .map(str::to_string)
        .ok_or_else(|| "could not decode for the check".to_string())
}

/// Copies every tag field and picture from one file into another's own format.
///
/// Built field by field rather than re-typed wholesale, because a re-type keeps
/// only the fields the new format has a name for - and the ones that matter to
/// a DJ, `ENERGY` and `MOOD` and the rest, are exactly the ones it has not.
fn copy_tags(src: &Path, dst: &Path) -> Result<(), String> {
    use lofty::prelude::*;
    use lofty::tag::{ItemKey, Tag};

    let from = lofty::read_from_path(src).map_err(|e| e.to_string())?;
    let Some(source) = from.primary_tag().or_else(|| from.first_tag()) else {
        return Ok(());
    };
    let mut to = lofty::read_from_path(dst).map_err(|e| e.to_string())?;
    let kind = to.primary_tag_type();

    let mut tag = Tag::new(kind);
    for item in source.items() {
        // A rating is a number in Vorbis and MP4 but a binary frame in ID3, so
        // copied as it is, a rated FLAC made an ID3 tag lofty refused to write -
        // and every other tag was lost with it. Written in the new format's own
        // form below instead.
        if *item.key() == ItemKey::Popularimeter {
            continue;
        }
        let key = match item.key() {
            ItemKey::Unknown(name) => noceur_tags::edit::custom_key(kind, name),
            k => k.clone(),
        };
        let item = lofty::tag::TagItem::new(key.clone(), item.value().clone());
        if matches!(key, ItemKey::Unknown(_)) {
            tag.insert_unchecked(item);
        } else {
            tag.push(item);
        }
    }
    for pic in source.pictures() {
        tag.push_picture(pic.clone());
    }

    to.insert_tag(tag);
    to.save_to_path(dst, lofty::config::WriteOptions::default())
        .map_err(|e| e.to_string())?;

    // Stars are not in the generic tag when they sit in an ID3 `POPM` frame,
    // so the copy above loses them from every WAV, AIFF and MP3 - and a WAV
    // converted to AIFF for rekordbox would arrive unrated. Written again in
    // whatever form the new format keeps them.
    if let Some(stars) = noceur_tags::read(src).ok().and_then(|m| m.rating) {
        let edit = noceur_tags::Edit {
            rating: noceur_tags::Change::Set(stars.to_string()),
            ..Default::default()
        };
        noceur_tags::edit::write(dst, &edit).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Where a converted file goes: the chosen folder, or beside the source.
fn output_for(src: &Path, format: Format, folder: &str) -> PathBuf {
    let dir = if folder.trim().is_empty() {
        src.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        PathBuf::from(folder.trim())
    };
    // Joined as text rather than with `with_extension`, which would treat the
    // last dot of "Title (feat. X)" as the start of an extension.
    let stem = src.file_stem().unwrap_or_default().to_string_lossy();
    dir.join(format!("{stem}.{}", format.ext()))
}

fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
    norm(a) == norm(b)
}

// ── Commands ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegInfo {
    /// Empty when none was found.
    pub path: String,
    pub version: String,
    /// What was typed in settings, so the box can show it.
    pub configured: String,
}

fn info() -> FfmpegInfo {
    let configured = Config::load(&Config::default_path()).ffmpeg_path;
    let found = find_ffmpeg(&configured);
    let version = found
        .as_ref()
        .and_then(|p| ffmpeg(p).arg("-version").output().ok())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or_default()
                .split(" Copyright")
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .unwrap_or_default();
    FfmpegInfo {
        path: found.map(|p| p.display().to_string()).unwrap_or_default(),
        version,
        configured,
    }
}

/// Which ffmpeg the converter would use.
#[tauri::command]
pub async fn ffmpeg_info() -> Result<FfmpegInfo, String> {
    tauri::async_runtime::spawn_blocking(info)
        .await
        .map_err(|e| e.to_string())
}

/// Sets where ffmpeg is - a file or the folder it is in - or clears it.
#[tauri::command]
pub async fn set_ffmpeg_path(path: String) -> Result<FfmpegInfo, String> {
    let file = Config::default_path();
    let mut config = Config::load(&file);
    config.ffmpeg_path = path.trim().to_string();
    config.save(&file)?;
    ffmpeg_info().await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub format: Format,
    pub depth: Want,
    /// Empty for beside each source.
    pub folder: String,
    pub replace: bool,
    pub verify: bool,
}

/// One file, as the dialog lists it before anything is written.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Planned {
    pub file_id: i64,
    pub name: String,
    pub from: String,
    pub to: String,
    pub output: String,
    pub exact: bool,
    /// Why it will be skipped; empty when it will not.
    pub problem: String,
}

/// What converting a file came to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Converted {
    pub file_id: i64,
    pub output: String,
    pub ok: bool,
    /// `Some(true)` when the audio was compared and matched.
    pub verified: Option<bool>,
    pub message: String,
}

fn locate(state: &AppState, file_id: i64) -> Result<PathBuf, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "index lock poisoned".to_string())?;
    let found = noceur_index::thumb::locate(&db, file_id)
        .map_err(|e| e.to_string())?
        .filter(|f| f.kind == 1)
        .ok_or_else(|| "not an audio file".to_string())?;
    Ok(found.path)
}

fn ffmpeg_or_err() -> Result<PathBuf, String> {
    find_ffmpeg(&Config::load(&Config::default_path()).ffmpeg_path).ok_or_else(|| {
        "ffmpeg was not found. Put ffmpeg.exe beside Noceur, or set its path in Settings.".into()
    })
}

fn plan_one(ff: &Path, src: &Path, file_id: i64, opts: &Options) -> Planned {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let output = output_for(src, opts.format, &opts.folder);
    let mut planned = Planned {
        file_id,
        name,
        from: String::new(),
        to: String::new(),
        output: output.display().to_string(),
        exact: false,
        problem: String::new(),
    };

    let stream = match probe(ff, src) {
        Ok(s) => s,
        Err(e) => {
            planned.problem = e;
            return planned;
        }
    };
    let r = recipe(&stream, opts.format, opts.depth);
    planned.from = stream.label();
    planned.to = format!("{} {}", opts.format.name(), r.depth.label());
    planned.exact = r.exact;
    planned.problem = if let Some(p) = r.problem {
        p
    } else if same_file(src, &output) {
        "already this format here".into()
    } else if output.exists() && !opts.replace {
        "already converted".into()
    } else {
        String::new()
    };
    planned
}

/// Lists what a conversion would do, file by file, without writing anything.
#[tauri::command]
pub async fn convert_plan(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    options: Options,
) -> Result<Vec<Planned>, String> {
    let ff = ffmpeg_or_err()?;
    let paths: Vec<(i64, Result<PathBuf, String>)> = file_ids
        .iter()
        .map(|&id| (id, locate(&state, id)))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|(id, path)| match path {
                Ok(p) => plan_one(&ff, &p, id, &options),
                Err(e) => Planned {
                    file_id: id,
                    name: String::new(),
                    from: String::new(),
                    to: String::new(),
                    output: String::new(),
                    exact: false,
                    problem: e,
                },
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

fn convert_one(ff: &Path, src: &Path, file_id: i64, opts: &Options) -> Converted {
    let plan = plan_one(ff, src, file_id, opts);
    let output = PathBuf::from(&plan.output);
    let fail = |message: String| Converted {
        file_id,
        output: plan.output.clone(),
        ok: false,
        verified: None,
        message,
    };
    if !plan.problem.is_empty() {
        return fail(plan.problem.clone());
    }
    let Ok(stream) = probe(ff, src) else {
        return fail("no audio stream".into());
    };
    let r = recipe(&stream, opts.format, opts.depth);

    if let Some(dir) = output.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return fail(format!("cannot create {}: {e}", dir.display()));
        }
    }

    // Written under a temporary name and moved into place at the end, so a
    // failed or interrupted conversion never leaves a half-file that looks
    // like a finished one.
    let tmp = output.with_file_name(format!(
        "{}.noceur-tmp.{}",
        output.file_stem().unwrap_or_default().to_string_lossy(),
        opts.format.ext()
    ));
    let out = ffmpeg(ff)
        .args(["-v", "error", "-y", "-i"])
        .arg(src)
        .args(["-map", "0:a:0", "-map_metadata", "-1"])
        .args(&r.args)
        .args(["-f", opts.format.muxer()])
        .arg(&tmp)
        .output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let _ = std::fs::remove_file(&tmp);
            let err = String::from_utf8_lossy(&o.stderr);
            return fail(err.lines().last().unwrap_or("ffmpeg failed").to_string());
        }
        Err(e) => return fail(format!("cannot run ffmpeg: {e}")),
    }

    let mut notes = Vec::new();
    if let Err(e) = copy_tags(src, &tmp) {
        notes.push(format!("tags not copied: {e}"));
    }

    let verified = if opts.verify && r.exact {
        let float = matches!(r.depth, Depth::Float(_));
        match (digest(ff, src, float), digest(ff, &tmp, float)) {
            (Ok(a), Ok(b)) if a == b => Some(true),
            (Ok(_), Ok(_)) => {
                let _ = std::fs::remove_file(&tmp);
                return Converted {
                    file_id,
                    output: plan.output.clone(),
                    ok: false,
                    verified: Some(false),
                    message: "the audio did not match the source; nothing was kept".into(),
                };
            }
            (Err(e), _) | (_, Err(e)) => {
                notes.push(e);
                None
            }
        }
    } else {
        None
    };

    if let Err(e) = std::fs::rename(&tmp, &output) {
        let _ = std::fs::remove_file(&tmp);
        return fail(format!("cannot move into place: {e}"));
    }

    // The source's date, when dates are being kept: a converted folder should
    // still sort by when the music arrived, not by when it was converted.
    if Config::load(&Config::default_path()).preserve_dates {
        if let Some(stamp) = crate::timestamp::Stamp::of(src) {
            stamp.restore(&output);
        }
    }

    Converted {
        file_id,
        output: plan.output,
        ok: true,
        verified,
        message: notes.join("; "),
    }
}

/// Converts one file. The dialog calls this once per file, so it can show
/// progress and stop between files.
#[tauri::command]
pub async fn convert_file(
    state: State<'_, AppState>,
    file_id: i64,
    options: Options,
) -> Result<Converted, String> {
    let ff = ffmpeg_or_err()?;
    let src = locate(&state, file_id)?;
    tauri::async_runtime::spawn_blocking(move || convert_one(&ff, &src, file_id, &options))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(line: &str) -> Stream {
        parse_stream(&format!("  Stream #0:0: Audio: {line}\n")).unwrap()
    }

    #[test]
    fn reads_what_ffmpeg_reports() {
        let s = stream("flac, 44100 Hz, stereo, s32 (24 bit)");
        assert_eq!(
            (s.codec.as_str(), s.rate, s.depth, s.lossless),
            ("flac", 44100, Depth::Int(24), true)
        );

        let s = stream("pcm_s16le ([1][0][0][0] / 0x0001), 48000 Hz, 2 channels, s16, 1536 kb/s");
        assert_eq!((s.rate, s.depth, s.lossless), (48000, Depth::Int(16), true));

        let s = stream("pcm_f32le ([3][0][0][0] / 0x0003), 44100 Hz, stereo, flt, 2822 kb/s");
        assert_eq!(s.depth, Depth::Float(32));

        let s =
            stream("alac (alac / 0x63616C61), 96000 Hz, stereo, s32p (24 bit), 840 kb/s (default)");
        assert_eq!((s.rate, s.depth), (96000, Depth::Int(24)));

        let s = stream("pcm_s32le ([1][0][0][0] / 0x0001), 44100 Hz, stereo, s32, 2822 kb/s");
        assert_eq!(s.depth, Depth::Int(32));

        let s = stream("mp3 (mp3float), 44100 Hz, stereo, fltp, 320 kb/s");
        assert!(!s.lossless);
    }

    #[test]
    fn source_depth_is_kept_and_counts_as_exact() {
        let src = stream("flac, 44100 Hz, stereo, s32 (24 bit)");
        let r = recipe(&src, Format::Wav, Want::Source);
        assert_eq!(r.depth, Depth::Int(24));
        assert!(r.exact);
        assert!(r.args.contains(&"pcm_s24le".to_string()));
        assert!(!r.args.iter().any(|a| a.contains("dither")));

        let r = recipe(&src, Format::Aiff, Want::Source);
        assert!(r.args.contains(&"pcm_s24be".to_string()));
    }

    #[test]
    fn float_goes_to_24_bit_for_flac_and_is_not_called_exact() {
        let src = stream("pcm_f32le, 44100 Hz, stereo, flt");
        let r = recipe(&src, Format::Flac, Want::Source);
        assert_eq!(r.depth, Depth::Int(24));
        assert!(!r.exact);
        // Float into WAV stays float, and so stays exact.
        let r = recipe(&src, Format::Wav, Want::Source);
        assert_eq!(r.depth, Depth::Float(32));
        assert!(r.exact);
    }

    #[test]
    fn narrowing_dithers() {
        let src = stream("flac, 44100 Hz, stereo, s32 (24 bit)");
        let r = recipe(&src, Format::Wav, Want::Sixteen);
        assert!(r.args.iter().any(|a| a.contains("dither_method")));
        assert!(!r.exact);
    }

    #[test]
    fn alac_refuses_32_bit() {
        let src = stream("pcm_s32le, 44100 Hz, stereo, s32");
        assert!(recipe(&src, Format::Alac, Want::Source).problem.is_some());
    }

    #[test]
    fn output_sits_beside_the_source_unless_a_folder_is_given() {
        let src = Path::new("/music/a/Track.flac");
        assert_eq!(
            output_for(src, Format::Wav, ""),
            PathBuf::from("/music/a/Track.wav")
        );
        assert_eq!(
            output_for(src, Format::Alac, "/out"),
            PathBuf::from("/out/Track.m4a")
        );
        let dotted = Path::new("/m/Title (feat. X).flac");
        assert_eq!(
            output_for(dotted, Format::Wav, ""),
            PathBuf::from("/m/Title (feat. X).wav")
        );
    }

    /// Runs ffmpeg for real: WAV into each format and back must be bit-exact,
    /// and tags must survive. Skipped where ffmpeg is not installed.
    #[test]
    fn a_real_round_trip_is_bit_exact_and_keeps_tags() {
        let Some(ff) = find_ffmpeg("") else { return };
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("tone.flac");
        let ok = ffmpeg(&ff)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=440:d=1",
                "-ac",
                "2",
            ])
            .args([
                "-c:a",
                "flac",
                "-sample_fmt",
                "s32",
                "-bits_per_raw_sample",
                "24",
            ])
            .arg(&src)
            .status()
            .unwrap();
        assert!(ok.success());
        {
            use lofty::prelude::*;
            let mut f = lofty::read_from_path(&src).unwrap();
            let mut tag = lofty::tag::Tag::new(f.primary_tag_type());
            tag.set_artist("Artist A, Artist B".into());
            tag.set_title("Title".into());
            tag.insert_unchecked(lofty::tag::TagItem::new(
                lofty::tag::ItemKey::Unknown("ENERGY".into()),
                lofty::tag::ItemValue::Text("4".into()),
            ));
            f.insert_tag(tag);
            f.save_to_path(&src, lofty::config::WriteOptions::default())
                .unwrap();
        }
        let tagged = noceur_tags::Edit {
            rating: noceur_tags::Change::Set("4".into()),
            comment: noceur_tags::Change::Set("7A - 5 #peak".into()),
            ..Default::default()
        };
        noceur_tags::edit::write(&src, &tagged).unwrap();

        // What reaches rekordbox: stars and comment tags, whatever the format.
        let carried = |path: &Path, what: &str| {
            let m = noceur_tags::read(path).unwrap();
            assert_eq!(m.rating, Some(4), "{what} stars");
            assert_eq!(m.comment.as_deref(), Some("7A - 5 #peak"), "{what} comment");
        };

        for format in [Format::Wav, Format::Aiff, Format::Alac] {
            let out = output_for(&src, format, "");
            let opts = Options {
                format,
                depth: Want::Source,
                folder: String::new(),
                replace: true,
                verify: true,
            };
            let done = convert_one(&ff, &src, 1, &opts);
            assert!(done.ok, "{format:?}: {}", done.message);
            assert!(!done.message.contains("not copied"), "{format:?}: {}", done.message);
            assert_eq!(done.verified, Some(true), "{format:?}");
            assert_eq!(
                probe(&ff, &out).unwrap().depth,
                Depth::Int(24),
                "{format:?}"
            );

            use lofty::prelude::*;
            let f = lofty::read_from_path(&out).unwrap();
            let t = f
                .primary_tag()
                .unwrap_or_else(|| panic!("{format:?}: no {:?} tag", f.primary_tag_type()));
            assert_eq!(
                t.artist().as_deref(),
                Some("Artist A, Artist B"),
                "{format:?}"
            );
            assert_eq!(t.title().as_deref(), Some("Title"), "{format:?}");
            let energy = noceur_tags::edit::custom_key(t.tag_type(), "ENERGY");
            assert_eq!(t.get_string(&energy), Some("4"), "{format:?} ENERGY");
            carried(&out, &format!("{format:?}"));
        }

        // WAV to AIFF, the way a library goes onto a rekordbox stick: the
        // stars sit in an ID3 POPM frame on both sides.
        let wav = output_for(&src, Format::Wav, "");
        let aiff_dir = dir.path().join("stick");
        std::fs::create_dir(&aiff_dir).unwrap();
        let opts = Options {
            format: Format::Aiff,
            depth: Want::Source,
            folder: aiff_dir.to_string_lossy().into_owned(),
            replace: true,
            verify: false,
        };
        let done = convert_one(&ff, &wav, 2, &opts);
        assert!(done.ok, "wav to aiff: {}", done.message);
        carried(&output_for(&wav, Format::Aiff, &opts.folder), "WAV to AIFF");
    }
}
