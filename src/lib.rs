mod error;
mod parser;
mod types;

pub use types::ZipReader;

#[cfg(test)]
mod tests {
    #[test]
    fn parse_test_files() {
        color_backtrace::install();

        use std::path::PathBuf;
        let zips_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("test-zips");

        // for name in &["test.zip", "zip64.zip", "unix.zip", "winxp.zip", "dd.zip"] {
        for name in &["zip64.zip", "readme.trailingzip"] {
            let test_file = zips_dir.join(name);
            let contents = std::fs::read(test_file).unwrap();
            super::ZipReader::new(&contents, contents.len()).unwrap();
        }
    }
}
