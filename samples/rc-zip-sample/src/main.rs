use clap::{App, Arg, ArgMatches, SubCommand};
use humansize::{file_size_opts::BINARY, FileSize};
use rc_zip::prelude::*;
use std::fmt;
use std::fs::File;

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
            SubCommand::with_name("info")
                .about("Show information about a ZIP file")
                .arg(
                    Arg::with_name("file")
                        .help("ZIP file to analyze")
                        .required(true)
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("list")
                .about("List files contained in a ZIP file")
                .arg(
                    Arg::with_name("file")
                        .help("ZIP file to list")
                        .required(true)
                        .index(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("extract")
                .about("Extract files contained in a ZIP file")
                .arg(
                    Arg::with_name("file")
                        .help("ZIP file to extract")
                        .required(true)
                        .index(1),
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
        println!("{} entries total", archive.entries().len());

        use std::collections::HashSet;
        let mut creator_versions = HashSet::<rc_zip::Version>::new();
        let mut reader_versions = HashSet::<rc_zip::Version>::new();
        for entry in archive.entries() {
            creator_versions.insert(entry.creator_version);
            reader_versions.insert(entry.reader_version);
        }
        println!("Creator versions: {:?}", creator_versions);
        println!("Reader versions: {:?}", reader_versions);
        println!("Detected encoding: {}", archive.encoding());
    }

    match matches.subcommand() {
        ("info", Some(matches)) => {
            let reader = File::open(matches.value_of("file").unwrap())?.read_zip()?;
            info(&reader);
        }
        ("list", Some(matches)) => {
            let reader = File::open(matches.value_of("file").unwrap())?.read_zip()?;
            info(&reader);
            println!("");

            use std::io::Write;
            use tabwriter::TabWriter;

            let mut stdout = std::io::stdout();
            let mut tw = TabWriter::new(&mut stdout);
            writeln!(&mut tw, "Mode\tName\tSize\tModified\tUID\tGID")?;

            for entry in reader.entries() {
                writeln!(
                    &mut tw,
                    "{mode}\t{name}\t{size}\t{modified}\t{uid}\t{gid}",
                    mode = entry.mode,
                    name = entry.name(),
                    size = entry.uncompressed_size.file_size(BINARY).unwrap(),
                    modified = entry.modified(),
                    uid = Optional(entry.uid),
                    gid = Optional(entry.gid),
                )
                .unwrap();
            }
            tw.flush().unwrap();
        }
        ("extract", Some(matches)) => {
            use std::io::Read;
            let file = File::open(matches.value_of("file").unwrap())?;
            let reader = file.read_zip()?;
            info(&reader);

            for entry in reader.entries() {
                println!("Extracting {}", entry.name());
                let mut contents = Vec::<u8>::new();
                entry
                    .reader(|offset| positioned_io::Cursor::new_pos(&file, offset))
                    .read_to_end(&mut contents)?;

                if let Ok(s) = std::str::from_utf8(&contents[..]) {
                    println!("contents = {:?}", s);
                } else {
                    println!("contents = {:?}", contents);
                }
            }
        }
        _ => {
            panic!("Invalid subcommand");
        }
    }

    Ok(())
}
