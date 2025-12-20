use std::{hint::black_box, time::Duration};

use divan::{bench, counter::ItemsCount, Bencher, Divan};
use rc_zip_corpus::test_cases;
use rc_zip_sync::ReadZip;

fn main() {
    Divan::default()
        .min_time(Duration::from_millis(500))
        .config_with_args()
        .main();
}

#[bench(args = ["meta.zip", "wine-zeroed.zip.bz2"])]
fn archive_entries(bencher: Bencher, name: &'static str) {
    let case = test_cases().into_iter().find(|c| c.name == name).unwrap();
    let zip_contents = case.bytes();
    let archive = zip_contents.read_zip().unwrap();
    let num_entries = ItemsCount::new(archive.entries().count());

    bencher
        .counter(num_entries)
        .bench(|| black_box(&zip_contents).read_zip().unwrap());
}
