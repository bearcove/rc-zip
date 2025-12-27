use std::{fs::File, mem};

use rc_zip::Entry;
use rc_zip_corpus::zips_dir;
use rc_zip_sync::ReadZip;

trait TotalSize: Sized {
    fn deep_size(&self) -> usize {
        0
    }

    fn total_size(&self) -> usize {
        mem::size_of::<Self>() + self.deep_size()
    }
}

impl TotalSize for Entry {
    fn deep_size(&self) -> usize {
        // the only deep data is from the `String`s
        self.name.deep_size() + self.comment.deep_size()
    }
}

impl TotalSize for String {
    fn deep_size(&self) -> usize {
        self.capacity()
    }
}

/// the size of an entry can start to add up when there are nearly a million of them
///
/// <https://github.com/bearcove/rc-zip/issues/146#issuecomment-3652248985>
#[test]
fn entry() {
    let zip_path = zips_dir().join("meta.zip");
    let zip_file = File::open(&zip_path).unwrap();
    let archive = zip_file.read_zip().unwrap();
    let shallow_dir = archive.by_name("rc-zip/src/").unwrap();
    let nested_file = archive
        .by_name("rc-zip/src/fsm/entry/bzip2_dec.rs")
        .unwrap();

    assert_eq!(mem::size_of::<Entry>(), 144);
    assert_eq!(shallow_dir.total_size(), 155);
    assert_eq!(nested_file.total_size(), 177);
}
