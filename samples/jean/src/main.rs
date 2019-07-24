use clap::{App, Arg, ArgMatches, SubCommand};
use humansize::{file_size_opts::BINARY, FileSize};
use rc_zip::{prelude::*, EntryContents};
use std::fmt;
use std::{
    fs::File,
    io::{self, Read},
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

fn main() {
    #[cfg(feature = "color-backtrace")]
    color_backtrace::install();
    #[cfg(feature = "env_logger")]
    env_logger::init();

    let matches = App::new("rc-zip sample")
        .subcommand(
            SubCommand::with_name("file")
                .about("Show information about a ZIP file")
                .arg(
                    Arg::with_name("zipfile")
                        .help("ZIP file to analyze")
                        .required(true)
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("ls")
                .about("List files contained in a ZIP file")
                .arg(
                    Arg::with_name("zipfile")
                        .help("ZIP file to list")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name("verbose")
                        .help("Show verbose information for each file")
                        .long("--verbose")
                        .short("v"),
                ),
        )
        .subcommand(
            SubCommand::with_name("unzip")
                .about("Extract files contained in a ZIP archive (unzip)")
                .arg(
                    Arg::with_name("zipfile")
                        .help("ZIP file to extract")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name("dir")
                        .help("Directory to extract to")
                        .default_value(".")
                        .short("-d"),
                ),
        )
        .subcommand(
            SubCommand::with_name("zip")
                .about("Add files to a ZIP archive (zip -r)")
                .arg(
                    Arg::with_name("zipfile")
                        .help("Path of the zip file to crate")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name("files")
                        .help("Files to add to the archive")
                        .required(true)
                        .multiple(true)
                        .index(2),
                ),
        )
        .get_matches();

    do_main(matches).unwrap();
}

fn do_main(matches: ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    fn info(archive: &rc_zip::Archive) {
        if let Some(comment) = archive.comment() {
            println!("Comment:\n{}", comment);
        }

        use std::collections::HashSet;
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
                rc_zip::EntryContents::Symlink(_) => {
                    num_symlinks += 1;
                }
                rc_zip::EntryContents::Directory(_) => {
                    num_dirs += 1;
                }
                rc_zip::EntryContents::File(f) => {
                    methods.insert(entry.method());
                    num_files += 1;
                    compressed_size += f.entry.compressed_size;
                    uncompressed_size += f.entry.uncompressed_size;
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
            uncompressed_size.file_size(BINARY).unwrap(),
            compressed_size as f64 / uncompressed_size as f64 * 100.0,
            num_files,
            num_dirs,
            num_symlinks,
        );
    }

    match matches.subcommand() {
        ("file", Some(matches)) => {
            let reader = File::open(matches.value_of("zipfile").unwrap())?.read_zip()?;
            info(&reader);
        }
        ("ls", Some(matches)) => {
            let zipfile = File::open(matches.value_of("zipfile").unwrap())?;
            let reader = zipfile.read_zip()?;
            let verbose = matches.is_present("verbose");

            for entry in reader.entries() {
                print!(
                    "{mode:>9} {size:>12} {name}",
                    mode = entry.mode,
                    name = entry.name().truncate_path(55),
                    size = entry.uncompressed_size.file_size(BINARY).unwrap(),
                );
                if verbose {
                    print!(
                        " {modified} {uid} {gid}",
                        modified = entry.modified(),
                        uid = Optional(entry.uid),
                        gid = Optional(entry.gid),
                    );

                    match entry.contents() {
                        rc_zip::EntryContents::Symlink(sl) => {
                            let mut target = String::new();
                            rc_zip::EntryReader::new(sl.entry, |offset| {
                                positioned_io::Cursor::new_pos(&zipfile, offset)
                            })
                            .read_to_string(&mut target)
                            .unwrap();
                            print!("\t{target}", target = target);
                        }
                        _ => {}
                    }
                }
                println!();
            }
        }
        ("unzip", Some(matches)) => {
            let zipfile = File::open(matches.value_of("zipfile").unwrap())?;
            let dir = std::path::Path::new(matches.value_of("dir").unwrap());
            let reader = zipfile.read_zip()?;

            let mut num_dirs = 0;
            let mut num_files = 0;
            let mut num_symlinks = 0;
            let mut uncompressed_size: u64 = 0;
            for entry in reader.entries() {
                if let EntryContents::File(f) = entry.contents() {
                    uncompressed_size += f.entry.uncompressed_size;
                }
            }

            let mut done_bytes: u64 = 0;
            use indicatif::{ProgressBar, ProgressStyle};
            let bar = ProgressBar::new(uncompressed_size);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("{eta_precise} [{bar:20.cyan/blue}] {wide_msg}")
                    .progress_chars("=>-"),
            );

            bar.enable_steady_tick(125);

            let start_time = std::time::SystemTime::now();
            for entry in reader.entries() {
                bar.set_message(entry.name());
                match entry.contents() {
                    EntryContents::Symlink(c) => {
                        num_symlinks += 1;
                        #[cfg(windows)]
                        {
                            let path = dir.join(c.entry.name());
                            std::fs::create_dir_all(
                                path.parent()
                                    .expect("all full entry paths should have parent paths"),
                            )?;
                            let mut entry_writer = File::create(path)?;
                            let mut entry_reader = c
                                .entry
                                .reader(|offset| positioned_io::Cursor::new_pos(&zipfile, offset));
                            std::io::copy(&mut entry_reader, &mut entry_writer)?;
                        }

                        #[cfg(not(windows))]
                        {
                            let path = dir.join(c.entry.name());
                            std::fs::create_dir_all(
                                path.parent()
                                    .expect("all full entry paths should have parent paths"),
                            )?;
                            if let Ok(_metadata) = std::fs::symlink_metadata(&path) {
                                std::fs::remove_file(&path)?;
                            }

                            let mut src = String::new();
                            c.entry
                                .reader(|offset| positioned_io::Cursor::new_pos(&zipfile, offset))
                                .read_to_string(&mut src)?;
                            std::os::unix::fs::symlink(src, &path)?;
                        }
                    }
                    EntryContents::Directory(c) => {
                        num_dirs += 1;
                        let path = dir.join(c.entry.name());
                        std::fs::create_dir_all(
                            path.parent()
                                .expect("all full entry paths should have parent paths"),
                        )?;
                    }
                    EntryContents::File(c) => {
                        num_files += 1;
                        let path = dir.join(c.entry.name());
                        std::fs::create_dir_all(
                            path.parent()
                                .expect("all full entry paths should have parent paths"),
                        )?;
                        let mut entry_writer = File::create(path)?;
                        let entry_reader = c
                            .entry
                            .reader(|offset| positioned_io::Cursor::new_pos(&zipfile, offset));
                        let before_entry_bytes = done_bytes;
                        let mut progress_reader =
                            ProgressRead::new(entry_reader, c.entry.uncompressed_size, |prog| {
                                bar.set_position(before_entry_bytes + prog.done);
                            });

                        let copied_bytes = std::io::copy(&mut progress_reader, &mut entry_writer)?;
                        done_bytes = before_entry_bytes + copied_bytes;
                    }
                }
            }
            bar.finish();
            let duration = start_time.elapsed()?;
            println!(
                "Extracted {} (in {} files, {} dirs, {} symlinks)",
                uncompressed_size.file_size(BINARY).unwrap(),
                num_files,
                num_dirs,
                num_symlinks
            );
            let seconds = (duration.as_millis() as f64) / 1000.0;
            let bps = (uncompressed_size as f64 / seconds) as u64;
            println!(
                "Overall extraction speed: {} / s",
                bps.file_size(BINARY).unwrap()
            );
        }
        ("zip", Some(matches)) => {
            let zipfile = matches.value_of("zipfile").unwrap();
            let files = matches.values_of("files").unwrap();
            println!("Should add {:?} to archive {:?}", files, zipfile);
            unimplemented!();
        }
        _ => {
            println!("{}", matches.usage());
            std::process::exit(1);
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
                name_tokens.extend(rest_tokens.into_iter());
                break name_tokens.join("/");
            }
            if rest_tokens.len() == 0 {
                name_tokens.extend(rest_tokens.into_iter());
                let name = name_tokens.join("/");
                break name.chars().take(limit - 3).collect::<String>() + "...";
            }
            let token = rest_tokens.pop_front().unwrap();
            match token.char_indices().skip(1).next() {
                Some((i, _)) => name_tokens.push(&token[..i]),
                None => name_tokens.push(token),
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Progress {
    done: u64,
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
