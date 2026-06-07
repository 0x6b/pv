use std::{
    cmp::Reverse,
    collections::HashMap,
    fmt::Write as _,
    fs::{File, Metadata, create_dir_all, read_to_string},
    io::{BufRead, BufReader, Cursor},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, ensure};
use clap::Parser;
use colored::Colorize;
use dirs::home_dir;
use jiff::{Timestamp, tz::TimeZone};
use shlex::split;
use skim::prelude::*;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// File path to open directly
    path: Option<PathBuf>,

    /// Command to open the file (receives absolute path as argument)
    #[arg(short, long, default_value = "gh mdp")]
    command: String,

    /// Base directory to search for markdown files (default: ~/.claude/plans, or PV_DIR env var)
    #[arg(short, long, env = "PV_DIR")]
    dir: Option<PathBuf>,

    /// Interactive mode: list files with fzf-style selection
    #[arg(short, long)]
    interactive: bool,
}

fn plans_dir() -> Result<PathBuf> {
    home_dir()
        .map(|h| h.join(".claude/plans"))
        .context("Failed to determine home directory")
}

fn markdown_files(dir: &Path) -> Result<Vec<(PathBuf, Metadata)>> {
    let mut files: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.into_path();
            let meta = path.metadata().ok()?;
            (path.extension().is_some_and(|ext| ext == "md") && meta.is_file())
                .then_some((path, meta))
        })
        .collect();
    files.sort_by_key(|b| Reverse(b.1.modified().ok()));
    Ok(files)
}

fn title(path: &Path) -> String {
    let Ok(file) = File::open(path) else {
        return String::new();
    };
    let mut first_non_empty = None;
    for line in BufReader::new(file).lines().take(10).map_while(Result::ok) {
        if let Some(heading) = line.strip_prefix("# ") {
            return heading.to_string();
        }
        if first_non_empty.is_none() && !line.is_empty() && line != "---" {
            first_non_empty = Some(line);
        }
    }
    first_non_empty.unwrap_or_default()
}

fn format_time(meta: &Metadata) -> String {
    meta.modified()
        .ok()
        .and_then(|t| Timestamp::try_from(t).ok())
        .map_or_else(
            || "unknown".into(),
            |ts| ts.to_zoned(TimeZone::system()).strftime("%Y-%m-%d %H:%M").to_string(),
        )
}

fn open(command: &str, path: &Path) -> Result<()> {
    let parts = split(command).context("Invalid command syntax")?;
    let (program, args) = parts.split_first().context("Empty command")?;
    let status = Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .with_context(|| format!("Failed to execute: {command}"))?;
    ensure!(status.success(), "Command exited with status: {status}");
    Ok(())
}

fn interactive(command: &str, files: &[(PathBuf, Metadata)]) -> Result<()> {
    ensure!(!files.is_empty(), "No markdown files found");

    // Map items back to paths by their ANSI-stripped match text.
    let mut input = String::new();
    let mut paths_by_text = HashMap::new();
    for (path, meta) in files {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let time = format_time(meta);
        let title = title(path);
        let _ = writeln!(input, "{} {} {}", time.blue(), title.bold(), name.dimmed());
        paths_by_text.insert(format!("{time} {title} {name}"), path.clone());
    }

    let preview_paths = paths_by_text.clone();
    let options = SkimOptionsBuilder::default()
        .height("100%".to_string())
        .multi(false)
        .reverse(true)
        .preview_fn(PreviewCallback::from(move |items: Vec<Arc<dyn SkimItem>>| {
            items
                .first()
                .and_then(|item| preview_paths.get(item.text().as_ref()))
                .and_then(|path| read_to_string(path).ok())
                .map(|content| content.lines().map(String::from).collect())
                .unwrap_or_default()
        }))
        .build()
        .map_err(|e| anyhow!("Failed to build skim options: {e}"))?;

    let items = SkimItemReader::new(SkimItemReaderOption::default().ansi(true))
        .of_bufread(Cursor::new(input));

    let output = Skim::run_with(options, Some(items)).map_err(|e| anyhow!("Skim failed: {e}"))?;

    if let Some(item) = output.selected_items.first().filter(|_| !output.is_abort)
        && let Some(path) = paths_by_text.get(item.text().as_ref())
    {
        open(command, path)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    if let Some(path) = &args.path {
        ensure!(path.exists(), "Path not found: {}", path.display());
        return open(&args.command, path);
    }

    let base = if let Some(dir) = args.dir {
        ensure!(
            dir.is_dir(),
            "Directory not found: {}. Please specify a valid directory with --dir.",
            dir.display()
        );
        dir
    } else {
        let dir = plans_dir()?;
        if !dir.is_dir() {
            create_dir_all(&dir).with_context(|| {
                format!(
                    "Plans directory {} does not exist and could not be created. \
                     Use --dir to specify a different directory.",
                    dir.display()
                )
            })?;
        }
        dir
    };
    let files = markdown_files(&base)?;

    if args.interactive {
        interactive(&args.command, &files)
    } else {
        let (path, _) = files.first().context("No markdown files found")?;
        open(&args.command, path)
    }
}
