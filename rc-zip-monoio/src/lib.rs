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

pub async fn read_zip_from_file(file: &File) -> Result<Archive, Error> {
    let meta = file.metadata().await?;
    let size = meta.len();
    let mut buf = vec![0u8; 128 * 1024];

    let mut fsm = ArchiveFsm::new(size);
    loop {
        if let Some(offset) = fsm.wants_read() {
            let dst = fsm.space();
            let max_read = dst.len().min(buf.len());
            let slice = IoBufMut::slice_mut(buf, 0..max_read);

            let (res, slice) = file.read_at(slice, offset).await;
            let n = res?;
            (dst[..n]).copy_from_slice(&slice[..n]);

            fsm.fill(n);
            buf = slice.into_inner();
        }

        fsm = match fsm.process()? {
            FsmResult::Done(archive) => break Ok(archive),
            FsmResult::Continue(fsm) => fsm,
        }
    }
}
