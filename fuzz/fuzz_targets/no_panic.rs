#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(archive) = rc_zip_sync::ReadZip::read_zip(&data) else {
        return;
    };
    for entry in archive.entries() {
        let _ = entry.sanitized_name();
        let _ = entry.reader();
    }
});
