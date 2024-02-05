use cfg_if::cfg_if;
use clap::{Parser, Subcommand};
use humansize::{format_size, BINARY};
use rc_zip::parse::{Archive, EntryKind, Method, Version};
use rc_zip_sync::{ReadZip, ReadZipStreaming};

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
    UnzipStreaming {
        zipfile: PathBuf,

        #[arg(long)]
        dir: Option<String>,
    },
}

fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    do_main(cli).unwrap();
}

fn do_main(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    fn info(archive: &Archive) {
        if !archive.comment().is_empty() {
            println!("Comment:\n{}", archive.comment());
        }

        let mut reader_versions = HashSet::<Version>::new();
        let mut methods = HashSet::<Method>::new();
        let mut compressed_size: u64 = 0;
        let mut uncompressed_size: u64 = 0;
        let mut num_dirs = 0;
        let mut num_symlinks = 0;
        let mut num_files = 0;

        for entry in archive.entries() {
            reader_versions.insert(entry.reader_version);
            match entry.kind() {
                EntryKind::Symlink => {
                    num_symlinks += 1;
                }
                EntryKind::Directory => {
                    num_dirs += 1;
                }
                EntryKind::File => {
                    methods.insert(entry.method);
                    num_files += 1;
                    compressed_size += entry.compressed_size;
                    uncompressed_size += entry.uncompressed_size;
                }
            }
        }
        println!("Versions: {:?}", reader_versions);
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
            info(&reader);

            for entry in reader.entries() {
                print!(
                    "{mode:>9} {size:>12} {name}",
                    mode = entry.mode,
                    name = if verbose {
                        Cow::Borrowed(&entry.name)
                    } else {
                        Cow::Owned(entry.name.truncate_path(55))
                    },
                    size = format_size(entry.uncompressed_size, BINARY),
                );
                if verbose {
                    print!(
                        " ({} compressed)",
                        format_size(entry.compressed_size, BINARY)
                    );
                    print!(
                        " {modified} {uid} {gid}",
                        modified = entry.modified,
                        uid = Optional(entry.uid),
                        gid = Optional(entry.gid),
                    );

                    if let EntryKind::Symlink = entry.kind() {
                        let mut target = String::new();
                        entry.reader().read_to_string(&mut target).unwrap();
                        print!("\t{target}", target = target);
                    }

                    print!("\t{:?}", entry.method);
                    if !entry.comment.is_empty() {
                        print!("\t{comment}", comment = entry.comment);
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
            let uncompressed_size = reader
                .entries()
                .map(|entry| entry.uncompressed_size)
                .sum::<u64>();

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
                let entry_name = match entry.sanitized_name() {
                    Some(name) => name,
                    None => continue,
                };

                pbar.set_message(entry_name.to_string());
                match entry.kind() {
                    EntryKind::Symlink => {
                        num_symlinks += 1;

                        cfg_if! {
                            if #[cfg(windows)] {
                                let path = dir.join(entry_name);
                                std::fs::create_dir_all(
                                    path.parent()
                                        .expect("all full entry paths should have parent paths"),
                                )?;
                                let mut entry_writer = File::create(path)?;
                                let mut entry_reader = entry.reader();
                                std::io::copy(&mut entry_reader, &mut entry_writer)?;
                            } else {
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
                    }
                    EntryKind::Directory => {
                        num_dirs += 1;
                        let path = dir.join(entry_name);
                        std::fs::create_dir_all(
                            path.parent()
                                .expect("all full entry paths should have parent paths"),
                        )?;
                    }
                    EntryKind::File => {
                        num_files += 1;
                        let path = dir.join(entry_name);
                        std::fs::create_dir_all(
                            path.parent()
                                .expect("all full entry paths should have parent paths"),
                        )?;
                        let mut entry_writer = File::create(path)?;
                        let entry_reader = entry.reader();
                        let before_entry_bytes = done_bytes;
                        let mut progress_reader =
                            ProgressReader::new(entry_reader, entry.uncompressed_size, |prog| {
                                pbar.set_position(before_entry_bytes + prog.done);
                            });

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
        Commands::UnzipStreaming { zipfile, dir, .. } => {
            let zipfile = File::open(zipfile)?;
            let dir = PathBuf::from(dir.unwrap_or_else(|| ".".into()));

            let mut num_dirs = 0;
            let mut num_files = 0;
            let mut num_symlinks = 0;

            let mut done_bytes: u64 = 0;
            use indicatif::{ProgressBar, ProgressStyle};
            let pbar = ProgressBar::new(100);
            pbar.set_style(
                ProgressStyle::default_bar()
                    .template("{eta_precise} [{bar:20.cyan/blue}] {wide_msg}")
                    .unwrap()
                    .progress_chars("=>-"),
            );

            let mut uncompressed_size = 0;
            pbar.enable_steady_tick(Duration::from_millis(125));

            let start_time = std::time::SystemTime::now();

            let mut entry_reader = zipfile.stream_zip_entries_throwing_caution_to_the_wind()?;
            loop {
                let entry_name = match entry_reader.entry().sanitized_name() {
                    Some(name) => name,
                    None => continue,
                };

                pbar.set_message(entry_name.to_string());
                match entry_reader.entry().kind() {
                    EntryKind::Symlink => {
                        num_symlinks += 1;

                        cfg_if! {
                            if #[cfg(windows)] {
                                let path = dir.join(entry_name);
                                std::fs::create_dir_all(
                                    path.parent()
                                        .expect("all full entry paths should have parent paths"),
                                )?;
                                let mut entry_writer = File::create(path)?;
                                let mut entry_reader = entry.reader();
                                std::io::copy(&mut entry_reader, &mut entry_writer)?;
                            } else {
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
                                entry_reader.read_to_string(&mut src)?;

                                // validate pointing path before creating a symbolic link
                                if src.contains("..") {
                                    continue;
                                }
                                std::os::unix::fs::symlink(src, &path)?;
                            }
                        }
                    }
                    EntryKind::Directory => {
                        num_dirs += 1;
                        let path = dir.join(entry_name);
                        std::fs::create_dir_all(
                            path.parent()
                                .expect("all full entry paths should have parent paths"),
                        )?;
                    }
                    EntryKind::File => {
                        num_files += 1;
                        let path = dir.join(entry_name);
                        std::fs::create_dir_all(
                            path.parent()
                                .expect("all full entry paths should have parent paths"),
                        )?;
                        let mut entry_writer = File::create(path)?;
                        let before_entry_bytes = done_bytes;
                        let total = entry_reader.entry().uncompressed_size;
                        let mut progress_reader =
                            ProgressReader::new(entry_reader, total, |prog| {
                                pbar.set_position(before_entry_bytes + prog.done);
                            });

                        let copied_bytes = std::io::copy(&mut progress_reader, &mut entry_writer)?;
                        uncompressed_size += copied_bytes;
                        done_bytes = before_entry_bytes + copied_bytes;
                        entry_reader = progress_reader.into_inner();
                    }
                }

                match entry_reader.finish()? {
                    Some(next_entry) => {
                        entry_reader = next_entry;
                    }
                    None => {
                        println!("End of archive!");
                        break;
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

#[derive(Clone, Copy)]
struct Progress {
    done: u64,
    #[allow(unused)]
    total: u64,
}

struct ProgressReader<F, R>
where
    R: io::Read,
    F: Fn(Progress),
{
    inner: R,
    callback: F,
    progress: Progress,
}

impl<F, R> ProgressReader<F, R>
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

impl<F, R> io::Read for ProgressReader<F, R>
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

impl<F, R> ProgressReader<F, R>
where
    R: io::Read,
    F: Fn(Progress),
{
    fn into_inner(self) -> R {
        self.inner
    }
}
