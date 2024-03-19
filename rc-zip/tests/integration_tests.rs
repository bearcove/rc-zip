use std::cmp;

use rc_zip::{
    corpus,
    fsm::{ArchiveFsm, FsmResult},
};

#[test]
fn state_machine() {
    corpus::install_test_subscriber();

    let cases = corpus::test_cases();
    let case = cases.iter().find(|x| x.name == "zip64.zip").unwrap();
    let bytes = case.bytes();

    let mut fsm = ArchiveFsm::new(bytes.len() as u64);

    let archive = 'read_zip: loop {
        if let Some(offset) = fsm.wants_read() {
            let increment = 128usize;
            let offset = offset as usize;
            let slice = if offset + increment > bytes.len() {
                &bytes[offset..]
            } else {
                &bytes[offset..offset + increment]
            };

            let len = cmp::min(slice.len(), fsm.space().len());
            fsm.space()[..len].copy_from_slice(&slice[..len]);
            match len {
                0 => panic!("EOF!"),
                read_bytes => {
                    fsm.fill(read_bytes);
                }
            }
        }

        fsm = match fsm.process() {
            Ok(res) => match res {
                FsmResult::Continue(fsm) => fsm,
                FsmResult::Done(archive) => break 'read_zip archive,
            },
            Err(err) => {
                panic!("{}", err)
            }
        }
    };

    // cool, we have the archive
    let _ = archive;
}
