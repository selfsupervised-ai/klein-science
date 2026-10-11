// provenance: original
//! `ksci-worker`: the R2 native shell. It reads request frames on stdin, calls `ksci_ops::invoke` and writes
//! response frames on stdout ([`ksci_wire::frame`]). It holds no logic: no environment reads, no clocks, no threads.
//!
//! Exit codes: 0 at a clean end of input; 2 on a frame error (an oversized length prefix, which is answered with a
//! bad-request frame first, a truncated frame, or a failed read); 1 when stdout cannot be written.

use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

use ksci_ops::OpError;
use ksci_wire::frame::{FrameError, Tag, read_request_frame, write_response_frame};

fn main() -> ExitCode {
    let mut input = io::stdin().lock();
    let mut output = BufWriter::new(io::stdout().lock());
    match serve(&mut input, &mut output) {
        Ok(code) => code,
        Err(_) => ExitCode::from(1),
    }
}

/// Answer frames until the input ends. `Err` only when a response frame cannot be written.
fn serve(input: &mut impl io::Read, output: &mut impl Write) -> io::Result<ExitCode> {
    loop {
        let request = match read_request_frame(input) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(ExitCode::SUCCESS),
            Err(e @ FrameError::TooLarge(_)) => {
                write_response_frame(output, Tag::BadRequest, e.to_string().as_bytes())?;
                return Ok(ExitCode::from(2));
            }
            Err(_) => return Ok(ExitCode::from(2)),
        };
        match ksci_ops::invoke(&request) {
            Ok(response) => write_response_frame(output, Tag::Ok, &response)?,
            Err(e) => write_response_frame(output, tag(&e), e.message().as_bytes())?,
        }
    }
}

fn tag(error: &OpError) -> Tag {
    match error {
        OpError::BadRequest(_) => Tag::BadRequest,
        OpError::UnsupportedOp(_) => Tag::UnsupportedOp,
        OpError::Internal(_) => Tag::Internal,
    }
}
