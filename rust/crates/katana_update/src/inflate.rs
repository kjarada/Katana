//! Raw deflate (RFC 1951) to bytes, with the output held to the size the signed manifest names.
//!
//! Every member of the payload zip is deflated alone, so a member's bytes - found by the
//! manifest's offset and csize - are a bare deflate stream with no zip header around them. The
//! output is CAPPED at the size the signed manifest says (`expected_size`): a stream that would
//! inflate to more is cut off and refused, so a hostile or corrupt member cannot fill the disk or
//! memory. Fewer bytes than expected is refused too.
//!
//! Why a cap of "expected + 1" per call. The inflater is asked for at most one byte more than the
//! signed size still to come, so a member that would inflate longer is noticed at the first
//! surplus byte and no more than that is ever produced; nothing is allocated from the signed size
//! either, the working buffer is a fixed block. A hostile member (a "zip bomb": a megabyte that
//! inflates to a gigabyte) therefore costs one block of memory and a few microseconds, whatever
//! it claims.
//!
//! CRC-32 is not checked: a member is a bare deflate stream, so there is none to compare with.
//! The caller's SHA-256 against the signed manifest is the integrity check.
//!
//! The C++ inflates with zlib; the port with miniz_oxide's core decompressor, driven so that it
//! refuses what zlib refuses. Its streaming form keeps a 32 KiB ring of history and so does not
//! check that a copy reaches no further back than the output began (zlib's "invalid distance too
//! far back"): it would copy from the ring's zeros. The core decompressor makes that check when it
//! is told the whole output is in one buffer, so the output is written into a buffer of RFC
//! 1951's 32 KiB window and one block, and the window is slid down when the block is full. Until
//! the first slide the buffer begins where the output began, so the check is exact; after it, at
//! least 32 KiB lies behind, and no distance reaches further.

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};

/// Receives the inflated bytes in order, in blocks. A failure stops the inflation and is
/// returned.
pub type InflateSink<'a> = dyn FnMut(&[u8]) -> Status + 'a;

// Output is handed to the sink in blocks of at most this many bytes. 64 KiB is the size at which
// a block copy dominates the call overhead; a larger block only holds more memory.
const BLOCK_BYTES: usize = 64 * 1024;

// The deflate window: no copy reaches further back than 32 KiB (RFC 1951, 2), so that much
// history is all the buffer keeps when it slides.
const HISTORY_BYTES: usize = 32 * 1024;

// inflate_raw_to_string refuses a signed size above this before it inflates anything: the result
// is held in memory whole, and a member that needs more than this is written to disk through
// inflate_raw with a sink. 256 MiB is far above any member "small" enough for the collected form
// (the largest file Katana ships is about 50 MiB) and far below the 2 GiB total an install may
// have (paths.rs).
const MAX_COLLECTED_BYTES: u64 = 256 * 1024 * 1024;

fn truncated(produced_so_far: u64) -> Error {
    make_error(
        ErrorCode::ParseFailure,
        "the compressed data ends before the end of the stream",
        format!("inflated {produced_so_far} bytes before it ended"),
    )
}

// The C++ names zlib's code and message here; miniz_oxide gives a status and no message.
fn not_deflate(status: TINFLStatus) -> Error {
    let why = match status {
        TINFLStatus::Failed => "invalid deflate data",
        TINFLStatus::FailedCannotMakeProgress => "no progress is possible",
        TINFLStatus::BadParam => "the inflater refused its parameters",
        TINFLStatus::Adler32Mismatch => "a checksum does not match",
        _ => "no progress although input and room remain",
    };
    make_error(
        ErrorCode::ParseFailure,
        "the compressed data is not a valid deflate stream",
        format!("inflate error: {why}"),
    )
}

/// ParseFailure for a stream that is not valid deflate, or ends early; InvalidArgument when it
/// would produce more than `expected_size` bytes or fewer; trailing bytes after the end of the
/// stream are InvalidArgument too (the member's csize is exact).
pub fn inflate_raw(compressed: &[u8], expected_size: u64, sink: &mut InflateSink<'_>) -> Status {
    let mut decompressor = Box::new(DecompressorOxide::new());
    // The history the decompressor may copy from, then the block it writes into.
    let mut window = vec![0u8; HISTORY_BYTES + BLOCK_BYTES];
    let mut out_pos = 0usize; // where the next output goes in `window`
    let mut consumed = 0usize; // bytes of `compressed` the decompressor has taken
    let mut produced = 0u64;
    let flags = inflate_flags::TINFL_FLAG_HAS_MORE_INPUT
        | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;

    loop {
        // At most one byte beyond the signed size still to come: see the top of the file.
        let remaining = expected_size - produced; // produced never exceeds it
        let room = if remaining >= BLOCK_BYTES as u64 {
            BLOCK_BYTES
        } else {
            remaining as usize + 1
        };
        if out_pos + room > window.len() {
            // room <= BLOCK_BYTES, so out_pos > HISTORY_BYTES here.
            window.copy_within(out_pos - HISTORY_BYTES..out_pos, 0);
            out_pos = HISTORY_BYTES;
        }

        let (status, read, made) = decompress(
            &mut decompressor,
            &compressed[consumed..],
            &mut window[..out_pos + room],
            out_pos,
            flags,
        );
        consumed += read;

        match status {
            TINFLStatus::Done | TINFLStatus::NeedsMoreInput | TINFLStatus::HasMoreOutput => {}
            failure => return Err(not_deflate(failure)),
        }

        if produced + made as u64 > expected_size {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the member inflates to more than the {expected_size} bytes its manifest entry \
                     names"
                ),
                "stopped at the first byte past the cap",
            ));
        }
        if made > 0 {
            sink(&window[out_pos..out_pos + made])?;
            produced += made as u64;
            out_pos += made;
        }

        if status == TINFLStatus::Done {
            if produced != expected_size {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "the member inflates to {produced} bytes, but its manifest entry names \
                         {expected_size}"
                    ),
                ));
            }
            if consumed != compressed.len() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "there are bytes after the end of the compressed stream",
                    format!("{} bytes", compressed.len() - consumed),
                ));
            }
            return Ok(());
        }

        // More input wanted, none left, and nothing made: the data ran out.
        if consumed == compressed.len() && made == 0 {
            return Err(truncated(produced));
        }
        if made == 0 && read == 0 {
            // No progress although both input and room remain: a state the decompressor is not
            // meant to reach; stop rather than spin.
            return Err(not_deflate(status));
        }
    }
}

/// The same, collected: for tests and for small members.
pub fn inflate_raw_to_string(compressed: &[u8], expected_size: u64) -> Result<Vec<u8>> {
    if expected_size > MAX_COLLECTED_BYTES {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the member is too large to hold in memory; inflate it to a sink",
            format!("{expected_size} bytes"),
        ));
    }
    // The signed size is believed only as far as the stream proves it: reserving it whole would
    // let a manifest entry that lies commit a quarter of a gigabyte before a byte was read.
    let mut out = Vec::with_capacity(expected_size.min(1 << 20) as usize);
    inflate_raw(compressed, expected_size, &mut |block: &[u8]| -> Status {
        out.extend_from_slice(block);
        Ok(())
    })?;
    Ok(out)
}
