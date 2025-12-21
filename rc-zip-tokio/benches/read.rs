use std::{hint::black_box, time::Duration};

use divan::{bench, counter::ItemsCount, Bencher, Divan};
use rc_zip_corpus::test_cases;
use rc_zip_tokio::ReadZip;

fn main() {
    Divan::default()
        .min_time(Duration::from_millis(500))
        .config_with_args()
        .main();
}

#[bench(args = ["meta.zip", "wine-zeroed.zip.bz2"])]
fn archive_entries(bencher: Bencher, name: &'static str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let case = test_cases().into_iter().find(|c| c.name == name).unwrap();
    let zip_contents = case.bytes();
    let num_entries =
        rt.block_on(async { zip_contents.read_zip().await.unwrap().entries().count() });
    let num_entries = ItemsCount::new(num_entries);

    bencher.counter(num_entries).bench_local(|| {
        rt.block_on(async { black_box(&zip_contents).read_zip().await.unwrap() });
    });
}
