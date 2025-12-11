use cfg_if::cfg_if;
use clap::{Parser, Subcommand};
use humansize::{format_size, BINARY};
use indicatif::{ProgressBar, ProgressStyle};
use rc_zip::{Archive, Entry, EntryKind};
use rc_zip_sync::{ArchiveHandle, ReadZip, ReadZipStreaming};

use std::{
    borrow::Cow,
    collections::HashSet,
    fmt,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[cfg(test)]
mod test;

struct Optional<T>(Option<T>);

impl<T> fmt::Display for Optional<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(x) = self.0.as_ref() {
            write!(f, "{}", x)
        } else {
            write!(f, "∅")
        }
    }
}

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    File {
        zipfile: PathBuf,
    },
    Ls {
        zipfile: PathBuf,

        #[arg(short, long)]
        verbose: bool,
    },
    Unzip {
        zipfile: PathBuf,

        #[arg(long)]
        dir: Option<PathBuf>,
    },
    UnzipStreaming {
        zipfile: PathBuf,

        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    do_main(cli).unwrap();
}

fn do_main(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::File { zipfile } => {
            let file = File::open(zipfile)?;
            let reader = file.read_zip()?;
            let mut stdout = io::stdout().lock();
            let _ = info(&mut stdout, &reader);
        }
        Commands::Ls { zipfile, verbose } => {
            let zipfile = File::open(zipfile)?;
            let reader = zipfile.read_zip()?;
            let mut stdout = io::stdout().lock();
            let _ = info(&mut stdout, &reader);
            let _ = list(&mut stdout, &reader, verbose);
        }
        Commands::Unzip { zipfile, dir } => unzip(&zipfile, dir.as_deref(), false)?,
        Commands::UnzipStreaming { zipfile, dir } => {
            unzip_streaming(&zipfile, dir.as_deref(), false)?
        }
    }

    Ok(())
}

fn info(out: &mut impl io::Write, archive: &Archive) -> io::Result<()> {
    if !archive.comment().is_empty() {
        writeln!(out, "Comment:\n{}", archive.comment())?;
    }

    let mut reader_versions = HashSet::new();
    let mut methods = HashSet::new();
    let mut compressed_size: u64 = 0;
    let mut stats = Stats::default();

    for entry in archive.entries() {
        reader_versions.insert(entry.reader_version);
        stats.inc_by_kind(entry.kind());
        if entry.kind().is_file() {
            methods.insert(entry.method);
            compressed_size += entry.compressed_size;
            stats.uncompressed_size += entry.uncompressed_size;
        }
    }
    writeln!(out, "Versions: {:?}", reader_versions)?;
    writeln!(
        out,
        "Encoding: {}, Methods: {:?}",
        archive.encoding(),
        methods
    )?;
    writeln!(
        out,
        "{} ({:.2}% compression) ({} files, {} dirs, {} symlinks)",
        format_size(stats.uncompressed_size, BINARY),
        compressed_size as f64 / stats.uncompressed_size as f64 * 100.0,
        stats.num_files,
        stats.num_dirs,
        stats.num_symlinks,
    )?;

    Ok(())
}

fn list(
    out: &mut impl io::Write,
    archive: &ArchiveHandle<'_, File>,
    verbose: bool,
) -> io::Result<()> {
    for entry in archive.entries() {
        write!(
            out,
            "{mode:>9} {size:>12} {name}",
            mode = entry.mode,
            name = if verbose {
                Cow::Borrowed(&entry.name)
            } else {
                Cow::Owned(entry.name.truncate_path(55))
            },
            size = format_size(entry.uncompressed_size, BINARY),
        )?;
        if verbose {
            write!(
                out,
                " ({} compressed)",
                format_size(entry.compressed_size, BINARY)
            )?;
            write!(
                out,
                " {modified} {uid} {gid}",
                modified = entry.modified,
                uid = Optional(entry.uid),
                gid = Optional(entry.gid),
            )?;

            if let EntryKind::Symlink = entry.kind() {
                let mut target = String::new();
                entry.reader().read_to_string(&mut target).unwrap();
                print!("\t{target}", target = target);
            }

            write!(out, "\t{:?}", entry.method)?;
            if !entry.comment.is_empty() {
                write!(out, "\t{comment}", comment = entry.comment)?;
            }
        }
        writeln!(out)?;
    }

    Ok(())
}

fn unzip(
    zipfile: &Path,
    dir: Option<&Path>,
    hide_progress: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let zipfile = File::open(zipfile)?;
    let dir = dir.unwrap_or_else(|| Path::new("."));
    let reader = zipfile.read_zip()?;

    let mut stats = Stats::default();
    let total_uncompressed_size = reader
        .entries()
        .map(|entry| entry.uncompressed_size)
        .sum::<u64>();

    let pbar = if hide_progress {
        ProgressBar::hidden()
    } else {
        let pbar = ProgressBar::new(total_uncompressed_size);
        pbar.set_style(
            ProgressStyle::default_bar()
                .template("{eta_precise} [{bar:20.cyan/blue}] {wide_msg}")
                .unwrap()
                .progress_chars("=>-"),
        );
        pbar.enable_steady_tick(Duration::from_millis(125));
        pbar
    };

    let start_time = Instant::now();
    for entry in reader.entries() {
        extract_entry(
            entry.to_owned(),
            &mut entry.reader(),
            dir,
            &pbar,
            &mut stats,
        )?;
    }
    pbar.finish();
    let duration = start_time.elapsed();
    println!(
        "Extracted {} (in {} files, {} dirs, {} symlinks)",
        format_size(stats.uncompressed_size, BINARY),
        stats.num_files,
        stats.num_dirs,
        stats.num_symlinks
    );
    let seconds = (duration.as_millis() as f64) / 1000.0;
    let bps = (stats.uncompressed_size as f64 / seconds) as u64;
    println!("Overall extraction speed: {} / s", format_size(bps, BINARY));

    Ok(())
}

fn unzip_streaming(
    zipfile: &Path,
    dir: Option<&Path>,
    hide_progress: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let zipfile = File::open(zipfile)?;
    let dir = dir.unwrap_or_else(|| Path::new("."));

    let mut stats = Stats::default();

    let pbar = if hide_progress {
        ProgressBar::hidden()
    } else {
        let pbar = ProgressBar::new_spinner();
        pbar.enable_steady_tick(Duration::from_millis(125));
        pbar
    };

    let start_time = Instant::now();

    let mut entry_reader = zipfile.stream_zip_entries_throwing_caution_to_the_wind()?;
    loop {
        extract_entry(
            entry_reader.entry().to_owned(),
            &mut entry_reader,
            dir,
            &pbar,
            &mut stats,
        )?;
        let Some(next_entry) = entry_reader.finish()? else {
            // End of archive!
            break;
        };
        entry_reader = next_entry;
    }
    pbar.finish();
    let duration = start_time.elapsed();
    println!(
        "Extracted {} (in {} files, {} dirs, {} symlinks)",
        format_size(stats.uncompressed_size, BINARY),
        stats.num_files,
        stats.num_dirs,
        stats.num_symlinks
    );
    let seconds = (duration.as_millis() as f64) / 1000.0;
    let bps = (stats.uncompressed_size as f64 / seconds) as u64;
    println!("Overall extraction speed: {} / s", format_size(bps, BINARY));

    Ok(())
}

fn extract_entry(
    entry: Entry,
    entry_reader: &mut impl io::Read,
    dir: &Path,
    pbar: &ProgressBar,
    stats: &mut Stats,
) -> rc_zip::Result<()> {
    let Some(entry_name) = entry.sanitized_name() else {
        return Ok(());
    };

    pbar.set_message(entry_name.to_string());
    let path = dir.join(entry_name);
    fs::create_dir_all(
        path.parent()
            .expect("all full entry paths should have parent paths"),
    )?;
    stats.inc_by_kind(entry.kind());
    match entry.kind() {
        EntryKind::Symlink => {
            cfg_if! {
                if #[cfg(windows)] {
                    let mut entry_writer = File::create(path)?;
                    io::copy(entry_reader, &mut entry_writer)?;
                } else {
                    if let Ok(metadata) = fs::symlink_metadata(&path) {
                        if metadata.is_file() {
                            fs::remove_file(&path)?;
                        }
                    }

                    let mut src = String::new();
                    entry_reader.read_to_string(&mut src)?;

                    // validate pointing path before creating a symbolic link
                    if src.contains("..") {
                        return Ok(());
                    }
                    std::os::unix::fs::symlink(src, &path)?;
                }
            }
        }
        EntryKind::Directory => fs::create_dir_all(&path)?,
        EntryKind::File => {
            let mut entry_writer = File::create(path)?;
            let mut progress_reader = pbar.wrap_read(entry_reader);

            let copied_bytes = io::copy(&mut progress_reader, &mut entry_writer)?;
            stats.uncompressed_size += copied_bytes;
        }
    }

    Ok(())
}

trait Truncate {
    fn truncate_path(&self, limit: usize) -> String;
}

impl Truncate for String {
    fn truncate_path(&self, limit: usize) -> String {
        let mut name_tokens: Vec<&str> = Vec::new();
        let mut rest_tokens: std::collections::VecDeque<&str> = self.split('/').collect();
        loop {
            let len_separators = name_tokens.len() + rest_tokens.len() - 1;
            let len_strings = name_tokens.iter().map(|x| x.len()).sum::<usize>()
                + rest_tokens.iter().map(|x| x.len()).sum::<usize>();
            if len_separators + len_strings < limit {
                name_tokens.extend(rest_tokens);
                break name_tokens.join("/");
            }
            if rest_tokens.is_empty() {
                name_tokens.extend(rest_tokens);
                let name = name_tokens.join("/");
                break name.chars().take(limit - 3).collect::<String>() + "...";
            }
            let token = rest_tokens.pop_front().unwrap();
            match token.char_indices().nth(1) {
                Some((i, _)) => name_tokens.push(&token[..i]),
                None => name_tokens.push(token),
            }
        }
    }
}

#[derive(Default)]
struct Stats {
    num_files: u32,
    num_dirs: u32,
    num_symlinks: u32,
    uncompressed_size: u64,
}

impl Stats {
    fn inc_by_kind(&mut self, kind: EntryKind) {
        match kind {
            EntryKind::File => self.num_files += 1,
            EntryKind::Directory => self.num_dirs += 1,
            EntryKind::Symlink => self.num_symlinks += 1,
        }
    }
}
