// provenance: original
//! Stdio frames between a host and a native worker: the one frame codec.
//!
//! - A request frame is a big-endian `u32` length, then that many bytes of a canonical JCS request.
//! - A response frame is a `u8` tag, a big-endian `u32` length, then the payload. The tag mirrors the WIT
//!   `result<list<u8>, error>`: [`Tag::Ok`] carries response JCS bytes, every other tag a UTF-8 error message.
//!
//! Every length is checked against [`MAX_DOCUMENT_BYTES`] before any payload byte is read, and a payload is read
//! incrementally, so a length prefix alone never makes the reader allocate the declared size.

use std::io::{self, ErrorKind, Read, Write};

use crate::MAX_DOCUMENT_BYTES;

/// The kind of a response frame, its first byte. Writer and reader share this type, so a frame with any other tag
/// cannot be written, and every frame written reads back (review N1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Tag {
    /// The response bytes of a request.
    Ok = 0,
    /// The request was malformed, non-canonical or oversized.
    BadRequest = 1,
    /// The request named an operation the plugin does not have.
    UnsupportedOp = 2,
    /// The plugin failed on a valid request.
    Internal = 3,
}

impl Tag {
    /// Every tag, in byte order.
    pub const ALL: [Tag; 4] = [Tag::Ok, Tag::BadRequest, Tag::UnsupportedOp, Tag::Internal];

    /// The tag's byte on the wire.
    pub const fn byte(self) -> u8 {
        self as u8
    }

    /// The tag a byte stands for, if any.
    pub const fn from_byte(byte: u8) -> Option<Tag> {
        match byte {
            0 => Some(Tag::Ok),
            1 => Some(Tag::BadRequest),
            2 => Some(Tag::UnsupportedOp),
            3 => Some(Tag::Internal),
            _ => None,
        }
    }
}

/// Why a frame could not be read.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The length prefix is above [`MAX_DOCUMENT_BYTES`]; no payload byte was read.
    #[error("frame length {0} is above the {MAX_DOCUMENT_BYTES}-byte cap")]
    TooLarge(u32),
    /// The input ended inside a frame.
    #[error("input ended inside a frame")]
    Truncated,
    /// A response frame carries a tag other than the four defined ones.
    #[error("unknown response tag {0}")]
    UnknownTag(u8),
    /// The underlying reader or writer failed.
    #[error("frame i/o: {0}")]
    Io(#[from] io::Error),
}

/// Fill `buf`, or report how many bytes arrived before end of input.
fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// A big-endian `u32` length checked against the cap, then exactly that many payload bytes.
fn read_payload(reader: &mut impl Read, len: [u8; 4]) -> Result<Vec<u8>, FrameError> {
    let len = u32::from_be_bytes(len);
    if u64::from(len) > MAX_DOCUMENT_BYTES as u64 {
        return Err(FrameError::TooLarge(len));
    }
    let mut payload = Vec::new();
    reader.take(u64::from(len)).read_to_end(&mut payload)?;
    if payload.len() as u64 != u64::from(len) {
        return Err(FrameError::Truncated);
    }
    Ok(payload)
}

/// Read one request frame. `Ok(None)` is a clean end of input before a length prefix.
pub fn read_request_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>, FrameError> {
    let mut len = [0u8; 4];
    match read_full(reader, &mut len)? {
        0 => Ok(None),
        4 => read_payload(reader, len).map(Some),
        _ => Err(FrameError::Truncated),
    }
}

/// Write one request frame and flush.
pub fn write_request_frame(writer: &mut impl Write, request: &[u8]) -> io::Result<()> {
    let len = frame_len(request)?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(request)?;
    writer.flush()
}

/// Read one response frame as `(tag, payload)`. `Ok(None)` is a clean end of input before a tag.
pub fn read_response_frame(reader: &mut impl Read) -> Result<Option<(Tag, Vec<u8>)>, FrameError> {
    let mut head = [0u8; 5];
    match read_full(reader, &mut head)? {
        0 => return Ok(None),
        5 => {}
        _ => return Err(FrameError::Truncated),
    }
    let tag = Tag::from_byte(head[0]).ok_or(FrameError::UnknownTag(head[0]))?;
    let payload = read_payload(reader, [head[1], head[2], head[3], head[4]])?;
    Ok(Some((tag, payload)))
}

/// Write one response frame and flush. A payload above [`MAX_DOCUMENT_BYTES`] is refused, so every frame written
/// can be read back.
///
/// Only the four defined tags can be written (review N1):
///
/// ```compile_fail,E0308
/// let mut out = Vec::new();
/// ksci_wire::frame::write_response_frame(&mut out, 4, b"").unwrap();
/// ```
pub fn write_response_frame(writer: &mut impl Write, tag: Tag, payload: &[u8]) -> io::Result<()> {
    let len = frame_len(payload)?;
    writer.write_all(&[tag.byte()])?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

fn frame_len(payload: &[u8]) -> io::Result<u32> {
    if payload.len() > MAX_DOCUMENT_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "payload of {} bytes is above the {MAX_DOCUMENT_BYTES}-byte cap",
                payload.len()
            ),
        ));
    }
    u32::try_from(payload.len()).map_err(|e| io::Error::new(ErrorKind::InvalidInput, e))
}
