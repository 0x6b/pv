use std::{
    fs::{Metadata, read_dir, read_to_string},
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, ensure};
use clap::Parser;
use dirs::home_dir;
use jiff::{Timestamp, tz::TimeZone};
use skim::prelude::*;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// File path to open directly
    path: Option<PathBuf>,

    /// Command to open the file (receives absolute path as argument)
    #[arg(short, long, default_value = "gh mdp")]
    command: String,

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
    let mut files: Vec<_> = read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let meta = e.metadata().ok()?;
            (path.extension().is_some_and(|ext| ext == "md") && meta.is_file())
                .then_some((path, meta))
        })
        .collect();
    files.sort_by(|a, b| b.1.modified().ok().cmp(&a.1.modified().ok()));
    Ok(files)
}

fn first_line(path: &Path) -> String {
    read_to_string(path)
        .ok()
        .and_then(|c| c.lines().next().map(str::to_string))
        .unwrap_or_default()
}

fn format_time(meta: &Metadata) -> String {
    meta.modified()
        .ok()
        .and_then(|t| Timestamp::try_from(t).ok())
        .map(|ts| ts.to_zoned(TimeZone::system()).strftime("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn open(command: &str, path: &Path) -> Result<()> {
    let mut parts = command.split_whitespace();
    let program = parts.next().context("Empty command")?;
    let status = Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .with_context(|| format!("Failed to execute: {command}"))?;
    ensure!(status.success(), "Command exited with status: {status}");
    Ok(())
}

fn interactive(command: &str, files: &[(PathBuf, Metadata)]) -> Result<()> {
    ensure!(!files.is_empty(), "No markdown files found");

    let input = files
        .iter()
        .map(|(path, meta)| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            format!(
                "\x1b[34m{}\x1b[0m \x1b[1m{}\x1b[0m \x1b[2m{name}\x1b[0m",
                format_time(meta),
                first_line(path),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let options = SkimOptionsBuilder::default()
        .height("50%".to_string())
        .multi(false)
        .reverse(true)
        .build()
        .map_err(|e| anyhow!("Failed to build skim options: {e}"))?;

    let items = SkimItemReader::new(SkimItemReaderOption::default().ansi(true))
        .of_bufread(Cursor::new(input));

    let output = Skim::run_with(options, Some(items)).map_err(|e| anyhow!("Skim failed: {e}"))?;

    if let Some(item) = output.selected_items.first().filter(|_| !output.is_abort)
        && let Some((path, _)) = files.get(item.get_index())
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

    let files = markdown_files(&plans_dir()?)?;

    if args.interactive {
        interactive(&args.command, &files)
    } else {
        let (path, _) = files.first().context("No markdown files found")?;
        open(&args.command, path)
    }
}
