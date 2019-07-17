use clap::{App, Arg, SubCommand};
use humansize::{file_size_opts::BINARY, FileSize};
use rc_zip::prelude::*;
use std::fmt;

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
    color_backtrace::install();
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
        .get_matches();

    fn read(file: &str) -> Result<rc_zip::Archive, Box<dyn std::error::Error>> {
        println!("Opening ({})", file);
        let file = std::fs::File::open(file)?;
        Ok(file.read_zip()?)
    }

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
            let reader = read(matches.value_of("file").expect("file missing")).unwrap();
            info(&reader);
        }
        ("list", Some(matches)) => {
            let reader = read(matches.value_of("file").expect("file missing")).unwrap();
            info(&reader);

            println!("");

            use std::io::Write;
            use tabwriter::TabWriter;

            let mut stdout = std::io::stdout();
            let mut tw = TabWriter::new(&mut stdout);
            writeln!(&mut tw, "Name\tSize\tModified\tUID\tGID").unwrap();

            for e in reader.entries() {
                writeln!(
                    &mut tw,
                    "{name}\t{size}\t{modified}\t{uid}\t{gid}",
                    name = e.name(),
                    size = e.uncompressed_size.file_size(BINARY).unwrap(),
                    modified = e.modified(),
                    uid = Optional(e.uid),
                    gid = Optional(e.gid),
                )
                .unwrap();
            }
            tw.flush().unwrap();
        }
        _ => {
            panic!("Invalid subcommand");
        }
    }
}
