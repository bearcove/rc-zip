use clap::{Parser, Subcommand};
use humansize::{format_size, BINARY};
use rc_zip::{prelude::*, EntryContents};

use std::{
    borrow::Cow,
    collections::HashSet,
    fmt,
    fs::File,
    io::{self, Read},
    path::PathBuf,
    time::Duration,
};

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

impl<T> fmt::Debug for Optional<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(x) = self.0.as_ref() {
            write!(f, "{:?}", x)
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
        dir: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    do_main(cli).unwrap();
}

fn do_main(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    fn info(archive: &rc_zip::Archive) {
        if let Some(comment) = archive.comment() {
            println!("Comment:\n{}", comment);
        }

        let mut creator_versions = HashSet::<rc_zip::Version>::new();
        let mut reader_versions = HashSet::<rc_zip::Version>::new();
        let mut methods = HashSet::<rc_zip::Method>::new();
        let mut compressed_size: u64 = 0;
        let mut uncompressed_size: u64 = 0;
        let mut num_dirs = 0;
        let mut num_symlinks = 0;
        let mut num_files = 0;

        for entry in archive.entries() {
            creator_versions.insert(entry.creator_version);
            reader_versions.insert(entry.reader_version);
            match entry.contents() {
                rc_zip::EntryContents::Symlink => {
                    num_symlinks += 1;
                }
                rc_zip::EntryContents::Directory => {
                    num_dirs += 1;
                }
                rc_zip::EntryContents::File => {
                    methods.insert(entry.method());
                    num_files += 1;
                    compressed_size += entry.inner.compressed_size;
                    uncompressed_size += entry.inner.uncompressed_size;
                }
            }
        }
        println!(
            "Version made by: {:?}, required: {:?}",
            creator_versions, reader_versions
        );
        println!("Encoding: {}, Methods: {:?}", archive.encoding(), methods);
        println!(
            "{} ({:.2}% compression) ({} files, {} dirs, {} symlinks)",
            format_size(uncompressed_size, BINARY),
            compressed_size as f64 / uncompressed_size as f64 * 100.0,
            num_files,
            num_dirs,
            num_symlinks,
        );
    }

    match cli.command {
        Commands::File { zipfile } => {
            let file = File::open(zipfile)?;
            let reader = file.read_zip()?;
            info(&reader);
        }
        Commands::Ls { zipfile, verbose } => {
            let zipfile = File::open(zipfile)?;
            let reader = zipfile.read_zip()?;

            for entry in reader.entries() {
                print!(
                    "{mode:>9} {size:>12} {name}",
                    mode = entry.mode,
                    name = if verbose {
                        Cow::from(entry.name())
                    } else {
                        Cow::from(entry.name().truncate_path(55))
                    },
                    size = format_size(entry.inner.uncompressed_size, BINARY),
                );
                if verbose {
                    print!(
                        " {modified} {uid} {gid}",
                        modified = entry.modified(),
                        uid = Optional(entry.uid),
                        gid = Optional(entry.gid),
                    );

                    if let rc_zip::EntryContents::Symlink = entry.contents() {
                        let mut target = String::new();
                        entry.reader().read_to_string(&mut target).unwrap();
                        print!("\t{target}", target = target);
                    }

                    if let Some(comment) = entry.comment() {
                        print!("\t{comment}", comment = comment);
                    }
                }
                println!();
            }
        }
        Commands::Unzip { zipfile, dir } => {
            let zipfile = File::open(zipfile)?;
            let dir = PathBuf::from(dir.unwrap_or_else(|| ".".into()));
            let reader = zipfile.read_zip()?;

            let mut num_dirs = 0;
            let mut num_files = 0;
            let mut num_symlinks = 0;
            let mut uncompressed_size: u64 = 0;
            for entry in reader.entries() {
                if let EntryContents::File = entry.contents() {
                    uncompressed_size += entry.inner.uncompressed_size;
                }
            }

            let mut done_bytes: u64 = 0;
            use indicatif::{ProgressBar, ProgressStyle};
            let pbar = ProgressBar::new(uncompressed_size);
            pbar.set_style(
                ProgressStyle::default_bar()
                    .template("{eta_precise} [{bar:20.cyan/blue}] {wide_msg}")
                    .unwrap()
                    .progress_chars("=>-"),
            );

            pbar.enable_steady_tick(Duration::from_millis(125));

            let start_time = std::time::SystemTime::now();
            for entry in reader.entries() {
                let entry_name = entry.name();
                let entry_name = match sanitize_entry_name(entry_name) {
                    Some(name) => name,
                    None => continue,
                };

                pbar.set_message(entry_name.to_string());
                match entry.contents() {
                    EntryContents::Symlink => {
                        num_symlinks += 1;
                        #[cfg(windows)]
                        {
                            let path = dir.join(entry_name);
                            std::fs::create_dir_all(
                                path.parent()
                                    .expect("all full entry paths should have parent paths"),
                            )?;
                            let mut entry_writer = File::create(path)?;
                            let mut entry_reader = entry.reader();
                            std::io::copy(&mut entry_reader, &mut entry_writer)?;
                        }

                        #[cfg(not(windows))]
                        {
                            let path = dir.join(entry_name);
                            std::fs::create_dir_all(
                                path.parent()
                                    .expect("all full entry paths should have parent paths"),
                            )?;
                            if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                                if metadata.is_file() {
                                    std::fs::remove_file(&path)?;
                                }
                            }

                            let mut src = String::new();
                            entry.reader().read_to_string(&mut src)?;

                            // validate pointing path before creating a symbolic link
                            if src.contains("..") {
                                continue;
                            }
                            std::os::unix::fs::symlink(src, &path)?;
                        }
                    }
                    EntryContents::Directory => {
                        num_dirs += 1;
                        let path = dir.join(entry_name);
                        std::fs::create_dir_all(
                            path.parent()
                                .expect("all full entry paths should have parent paths"),
                        )?;
                    }
                    EntryContents::File => {
                        num_files += 1;
                        let path = dir.join(entry_name);
                        std::fs::create_dir_all(
                            path.parent()
                                .expect("all full entry paths should have parent paths"),
                        )?;
                        let mut entry_writer = File::create(path)?;
                        let entry_reader = entry.reader();
                        let before_entry_bytes = done_bytes;
                        let mut progress_reader = ProgressRead::new(
                            entry_reader,
                            entry.inner.uncompressed_size,
                            |prog| {
                                pbar.set_position(before_entry_bytes + prog.done);
                            },
                        );

                        let copied_bytes = std::io::copy(&mut progress_reader, &mut entry_writer)?;
                        done_bytes = before_entry_bytes + copied_bytes;
                    }
                }
            }
            pbar.finish();
            let duration = start_time.elapsed()?;
            println!(
                "Extracted {} (in {} files, {} dirs, {} symlinks)",
                format_size(uncompressed_size, BINARY),
                num_files,
                num_dirs,
                num_symlinks
            );
            let seconds = (duration.as_millis() as f64) / 1000.0;
            let bps = (uncompressed_size as f64 / seconds) as u64;
            println!("Overall extraction speed: {} / s", format_size(bps, BINARY));
        }
    }

    Ok(())
}

trait Truncate {
    fn truncate_path(&self, limit: usize) -> String;
}

impl Truncate for &str {
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

#[derive(Clone, Copy)]
struct Progress {
    done: u64,
    #[allow(unused)]
    total: u64,
}

struct ProgressRead<F, R>
where
    R: io::Read,
    F: Fn(Progress),
{
    inner: R,
    callback: F,
    progress: Progress,
}

impl<F, R> ProgressRead<F, R>
where
    R: io::Read,
    F: Fn(Progress),
{
    fn new(inner: R, total: u64, callback: F) -> Self {
        Self {
            inner,
            callback,
            progress: Progress { total, done: 0 },
        }
    }
}

impl<F, R> io::Read for ProgressRead<F, R>
where
    R: io::Read,
    F: Fn(Progress),
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let res = self.inner.read(buf);
        if let Ok(n) = res {
            self.progress.done += n as u64;
            (self.callback)(self.progress);
        }
        res
    }
}

/// Sanitize zip entry names: skip entries with traversed/absolute path to
/// mitigate zip slip, and strip absolute prefix on entries pointing to root
/// path.
fn sanitize_entry_name(name: &str) -> Option<&str> {
    // refuse entries with traversed/absolute path to mitigate zip slip
    if name.contains("..") {
        return None;
    }

    #[cfg(windows)]
    {
        if name.contains(":\\") || name.starts_with("\\") {
            return None;
        }
        Some(name)
    }

    #[cfg(not(windows))]
    {
        // strip absolute prefix on entries pointing to root path
        let mut entry_chars = name.chars();
        let mut name = name;
        while name.starts_with('/') {
            entry_chars.next();
            name = entry_chars.as_str()
        }
        Some(name)
    }
}
