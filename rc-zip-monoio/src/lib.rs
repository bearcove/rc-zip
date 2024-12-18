//! A library for reading zip files asynchronously using monoio I/O traits,
//! based on top of [rc-zip](https://crates.io/crates/rc-zip).
//!
//! See also:
//!
//!   * [rc-zip-sync](https://crates.io/crates/rc-zip-sync) for using std I/O traits
//!   * [rc-zip-tokio](https://crates.io/crates/rc-zip-tokio) for using tokio traits

use monoio::{buf::IoBufMut, fs::File};
use rc_zip::{
    error::Error,
    fsm::{ArchiveFsm, FsmResult},
    parse::Archive,
};

macro_rules! dbgln {
    ($($tt:tt)*) => {
        eprintln!($($tt)*);
    }
}

pub async fn read_zip_from_file(file: &File) -> Result<Archive, Error> {
    dbgln!("Starting to read zip from file...");
    let meta = file.metadata().await?;
    let size = meta.len();
    dbgln!("File size: {size}");
    let mut buf = vec![0u8; 256 * 1024].into_boxed_slice();

    let mut fsm = ArchiveFsm::new(size);
    loop {
        dbgln!("Entering loop...");
        if let Some(offset) = fsm.wants_read() {
            dbgln!("FSM wants read at offset: {offset}");
            let dst = fsm.space();
            let max_read = dst.len().min(buf.len());
            dbgln!(
                "Calculated max_read: {max_read}, dst.len: {}, buf.len: {}",
                dst.len(),
                buf.len()
            );
            let slice = IoBufMut::slice_mut(buf, 0..max_read);

            let (res, slice) = file.read_at(slice, offset).await;
            dbgln!("Read result: {:?}", res);
            let n = res?;
            dbgln!("Number of bytes read: {n}");
            (dst[..n]).copy_from_slice(&slice[..n]);

            fsm.fill(n);
            dbgln!("FSM filled with {n} bytes");
            buf = slice.into_inner();
        } else {
            dbgln!("FSM does not want to read more data.");
        }

        fsm = match fsm.process()? {
            FsmResult::Done(archive) => {
                dbgln!("FSM processing done.");
                break Ok(archive);
            }
            FsmResult::Continue(fsm) => {
                dbgln!("FSM wants to continue.");
                fsm
            }
        }
    }
}
