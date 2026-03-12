use std::{
    fs::Metadata,
    io::{BufRead, BufReader, Cursor},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, ensure};
use clap::Parser;
use colored::Colorize;
use dirs::home_dir;
use jiff::{Timestamp, tz::TimeZone};
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
    ensure!(dir.is_dir(), "Directory not found: {}", dir.display());
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
    files.sort_by(|a, b| b.1.modified().ok().cmp(&a.1.modified().ok()));
    Ok(files)
}

fn first_line(path: &Path) -> String {
    std::fs::File::open(path)
        .ok()
        .and_then(|f| BufReader::new(f).lines().next()?.ok())
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
    let parts = shlex::split(command).context("Invalid command syntax")?;
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

    let input = files
        .iter()
        .map(|(path, meta)| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            format!(
                "{} {} {}",
                format_time(meta).blue(),
                first_line(path).bold(),
                name.dimmed(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let paths: Vec<_> = files.iter().map(|(p, _)| p.clone()).collect();
    let options = SkimOptionsBuilder::default()
        .height("50%".to_string())
        .multi(false)
        .reverse(true)
        .preview_fn(PreviewCallback::from(move |items: Vec<Arc<dyn SkimItem>>| {
            items
                .first()
                .and_then(|item| paths.get(item.get_index()))
                .and_then(|path| std::fs::read_to_string(path).ok())
                .map(|content| content.lines().map(String::from).collect())
                .unwrap_or_default()
        }))
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

    let base = args.dir.unwrap_or(plans_dir()?);
    let files = markdown_files(&base)?;

    if args.interactive {
        interactive(&args.command, &files)
    } else {
        let (path, _) = files.first().context("No markdown files found")?;
        open(&args.command, path)
    }
}
