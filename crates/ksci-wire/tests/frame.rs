// provenance: original
//! The stdio frame codec.

use ksci_wire::frame::{read_response_frame, write_response_frame};

/// Review N1: every response frame the writer accepts reads back as written.
#[test]
fn frame_written_reads_back() {
    for tag in 0..=u8::MAX {
        let mut bytes = Vec::new();
        if write_response_frame(&mut bytes, tag, b"payload").is_ok() {
            let read = read_response_frame(&mut bytes.as_slice());
            assert!(
                matches!(read, Ok(Some((t, ref p))) if t == tag && p == b"payload"),
                "tag {tag}: {read:?}"
            );
        }
    }
}
