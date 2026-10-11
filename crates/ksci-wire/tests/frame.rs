// provenance: original
//! The stdio frame codec.

use ksci_wire::frame::{FrameError, Tag, read_response_frame, write_response_frame};

/// Review N1: every response frame the writer accepts reads back as written. Only [`Tag`] values can be written; a
/// frame whose first byte is no tag (which no writer produces) is refused on read.
#[test]
fn frame_written_reads_back() {
    assert_eq!(
        Tag::ALL.map(Tag::byte),
        [0, 1, 2, 3],
        "the wire bytes of the tags"
    );
    for byte in 0..=u8::MAX {
        match Tag::from_byte(byte) {
            Some(tag) => {
                assert_eq!(tag.byte(), byte);
                let mut bytes = Vec::new();
                write_response_frame(&mut bytes, tag, b"payload").unwrap();
                let read = read_response_frame(&mut bytes.as_slice());
                assert!(
                    matches!(read, Ok(Some((t, ref p))) if t == tag && p == b"payload"),
                    "tag {byte}: {read:?}"
                );
            }
            None => {
                let frame = [byte, 0, 0, 0, 0];
                let read = read_response_frame(&mut frame.as_slice());
                assert!(
                    matches!(read, Err(FrameError::UnknownTag(b)) if b == byte),
                    "byte {byte}: {read:?}"
                );
            }
        }
    }
}
