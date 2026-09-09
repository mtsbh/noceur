//! Debug CLI for the Noceur engine. Not the shipping interface.

use clap::{Parser, Subcommand, ValueEnum};
use noceur_index::query::{count, list, ListOptions, Show, SortColumn};
use noceur_index::volume::{upsert, VolumeKind};
use noceur_index::{probe_dir, scan_dir, Db};
use noceur_tags::{KeyNotation, MusicalKey};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "noceur", about = "Noceur engine debug CLI")]
struct Cli {
    /// Index database path. Defaults to an in-memory database.
    #[arg(long)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan a directory and print it in the app's sort order.
    Ls {
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = Sort::Name)]
        sort: Sort,
        #[arg(long)]
        desc: bool,
        #[arg(long)]
        audio_only: bool,
        /// Display notation for the key column.
        #[arg(long, value_enum, default_value_t = Notation::Camelot)]
        notation: Notation,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Sort {
    Name,
    Size,
    Modified,
    Bpm,
    Key,
    Duration,
    Artist,
    Title,
    Album,
}

impl From<Sort> for SortColumn {
    fn from(s: Sort) -> Self {
        match s {
            Sort::Name => SortColumn::Name,
            Sort::Size => SortColumn::Size,
            Sort::Modified => SortColumn::Modified,
            Sort::Bpm => SortColumn::Bpm,
            Sort::Key => SortColumn::Key,
            Sort::Duration => SortColumn::Duration,
            Sort::Artist => SortColumn::Artist,
            Sort::Title => SortColumn::Title,
            Sort::Album => SortColumn::Album,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Notation {
    Camelot,
    OpenKey,
    Classical,
}

impl From<Notation> for KeyNotation {
    fn from(n: Notation) -> Self {
        match n {
            Notation::Camelot => KeyNotation::Camelot,
            Notation::OpenKey => KeyNotation::OpenKey,
            Notation::Classical => KeyNotation::Classical,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let db = match &cli.db {
        Some(p) => Db::open(p)?,
        None => Db::open_in_memory()?,
    };

    match cli.command {
        Command::Ls {
            path,
            sort,
            desc,
            audio_only,
            notation,
        } => {
            let path = path.canonicalize()?;
            let vol = upsert(&db, "cli-local", VolumeKind::Fixed, "local", "/")?;

            let started = std::time::Instant::now();
            let dir_id = scan_dir(&db, vol, &path)?;
            let scanned = started.elapsed();
            probe_dir(&db, dir_id)?;
            let probed = started.elapsed();

            let opts = ListOptions {
                sort: sort.into(),
                desc,
                show: if audio_only {
                    Show::AudioOnly
                } else {
                    Show::Everything
                },
                ..Default::default()
            };
            let total = count(&db, dir_id, opts.clone())?;
            let rows = list(&db, dir_id, opts)?;

            for r in &rows {
                let marker = match r.kind {
                    0 => "/",
                    2 => " ",
                    _ => " ",
                };
                let bpm = r
                    .bpm
                    .map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "—".into());
                let key = r
                    .key_norm
                    .and_then(|v| MusicalKey::from_canonical(v as i32))
                    .map(|k| k.format(notation.into()))
                    .or_else(|| r.key_tag.clone())
                    .unwrap_or_else(|| "—".into());
                let dur = r
                    .duration_ms
                    .map(|ms| format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60))
                    .unwrap_or_else(|| "—".into());
                let state = if r.state == "ok" {
                    String::new()
                } else {
                    format!("  [{}]", r.state)
                };

                println!(
                    "{marker} {:<44} {:>7} {:>5} {:>7}  {}{}",
                    truncate(&r.name, 44),
                    bpm,
                    key,
                    dur,
                    r.artist.clone().unwrap_or_default(),
                    state
                );
            }

            eprintln!(
                "\n{total} entries · scan {:.0}ms · probe {:.0}ms",
                scanned.as_secs_f64() * 1000.0,
                (probed - scanned).as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
