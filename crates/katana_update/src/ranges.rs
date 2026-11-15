//! Which bytes of the release's payload zip to ask for, the text of the request that asks, and
//! the check of the answer.
//!
//! The zip holds every install file as one deflate member at a known offset (the manifest's
//! offset and csize); the producer orders the members program, lib, data, seed, so the changed
//! programs - the bulk of a monthly update - are adjacent. The payload is never fetched whole.
//!
//! Nearby members are merged into one request, because each request costs about 150 ms of
//! latency and a gap of 1 MiB costs about as much to fetch again at 6 MB/s (both measured against
//! the release host on 2026-10-08). A merged range is at most 32 MiB, so a server that ignores
//! `Range` and answers with the whole file is cut off after that much (maxBytes equals the range
//! length).
//!
//! One range per request: multi-range requests were answered with 501 by the release storage
//! host when measured.

use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, trimmed};
use katana_core::{ErrorCode, Result, Status};

/// Two members merge when the hole between them is at most this; a merged range never exceeds
/// [`RANGE_MAX_LENGTH`].
pub const RANGE_MERGE_GAP: u64 = 1024 * 1024;
pub const RANGE_MAX_LENGTH: u64 = 32 * 1024 * 1024;

/// One manifest file's compressed bytes in the payload.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangeMember {
    /// Into the manifest's files.
    pub file_index: usize,
    pub offset: u64,
    /// csize.
    pub length: u64,
}

/// A request: bytes [offset, offset + length) of the payload, and the members whose data lies in
/// it, in offset order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MergedRange {
    pub offset: u64,
    pub length: u64,
    pub members: Vec<RangeMember>,
}

/// Sorts by offset, then merges neighbours whose gap is at most `max_gap`, never letting a range
/// exceed `max_length` (a single member larger than `max_length` is a range of its own,
/// unsplit). Overlapping members (a malformed manifest) are merged, not duplicated. A member with
/// length 0 is DROPPED - it has no bytes to ask for, and the manifest parser (csize >= 2) never
/// lets one through. Total over what remains: every member with length > 0 is in exactly one
/// range, and a dropped member is in none. The C++ defaults are [`RANGE_MERGE_GAP`] and
/// [`RANGE_MAX_LENGTH`].
pub fn merge_ranges(
    mut members: Vec<RangeMember>,
    max_gap: u64,
    max_length: u64,
) -> Vec<MergedRange> {
    // A member with no bytes has nothing to ask for; it must not bridge two others either.
    members.retain(|member| member.length != 0);
    // Offset order; ties by length then index so the result does not depend on the order the
    // caller listed the members in.
    members.sort_by(|a, b| {
        a.offset
            .cmp(&b.offset)
            .then(a.length.cmp(&b.length))
            .then(a.file_index.cmp(&b.file_index))
    });

    let mut ranges: Vec<MergedRange> = Vec::new();
    let mut current_end = 0u64; // end of the last range, when the last range can grow
    let mut current_can_grow = false;
    for member in members {
        let Some(member_end) = member.offset.checked_add(member.length) else {
            // Cannot be a part of any real payload (offset + length wraps): its own range,
            // unchanged, never merged. range_header_value refuses to ask for it.
            ranges.push(MergedRange {
                offset: member.offset,
                length: member.length,
                members: vec![member],
            });
            current_can_grow = false;
            continue;
        };
        if current_can_grow && let Some(current) = ranges.last_mut() {
            // Members are sorted by offset, so member.offset >= current.offset; a member that
            // starts inside the range (an overlap) has gap 0.
            let gap = member.offset.saturating_sub(current_end);
            let new_end = current_end.max(member_end);
            // The cap wins over everything, overlap included: the range length is what a server
            // that ignores `Range` is cut off at.
            if gap <= max_gap && new_end - current.offset <= max_length {
                current.length = new_end - current.offset;
                current.members.push(member);
                current_end = new_end;
                continue;
            }
        }
        ranges.push(MergedRange {
            offset: member.offset,
            length: member.length,
            members: vec![member],
        });
        current_end = member_end;
        current_can_grow = true;
    }
    ranges
}

// ---- the request and the check of its answer (the transport, feed, uses these) ----------------

/// The reason a server that ignores `Range` is refused with: the message of the Unsupported error
/// check_range_response returns, and the sentence the dialog shows.
pub const PARTIAL_DOWNLOADS_REFUSED: &str = "this network does not allow partial downloads";

/// The value of the `Range` header for bytes [offset, offset + length): "bytes=<first>-<last>",
/// <last> inclusive (RFC 9110, 14.1.2). InvalidArgument for length 0 (no byte to ask for) and for
/// a range whose end does not fit 64 bits.
pub fn range_header_value(offset: u64, length: u64) -> Result<String> {
    match offset.checked_add(length) {
        Some(end) if length != 0 => Ok(format!("bytes={offset}-{}", end - 1)),
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            "there is no valid byte range to ask for",
            format!("offset {offset}, length {length}"),
        )),
    }
}

// 1 to 20 ASCII digits that fit 64 bits, consumed from the front of `text`.
fn take_number(text: &mut &[u8]) -> Option<u64> {
    let slice = *text;
    let mut digits = 0usize;
    let mut result = 0u64;
    while digits < slice.len() && slice[digits].is_ascii_digit() {
        let digit = u64::from(slice[digits] - b'0');
        result = result.checked_mul(10)?.checked_add(digit)?;
        digits += 1;
    }
    if digits == 0 {
        return None;
    }
    *text = &slice[digits..];
    Some(result)
}

fn take_literal(text: &mut &[u8], c: u8) -> bool {
    let slice = *text;
    match slice.split_first() {
        Some((&first, rest)) if first == c => {
            *text = rest;
            true
        }
        _ => false,
    }
}

// "bytes <first>-<last>/<total>" or "bytes <first>-<last>/*" (RFC 9110, 14.4). The unit is
// case-insensitive. `total` is None for "*".
struct ContentRange {
    first: u64,
    last: u64,
    total: Option<u64>,
}

fn parse_content_range(header: &str) -> Option<ContentRange> {
    let mut text = trimmed(header.as_bytes());
    const UNIT: &[u8] = b"bytes";
    if text.len() < UNIT.len() || !equals_ignoring_case(&text[..UNIT.len()], UNIT) {
        return None;
    }
    text = &text[UNIT.len()..];
    if !take_literal(&mut text, b' ') {
        return None;
    }
    let first = take_number(&mut text)?;
    if !take_literal(&mut text, b'-') {
        return None;
    }
    let last = take_number(&mut text)?;
    if !take_literal(&mut text, b'/') {
        return None;
    }
    let total = if take_literal(&mut text, b'*') {
        None
    } else {
        Some(take_number(&mut text)?)
    };
    if !text.is_empty() || first > last {
        return None;
    }
    Some(ContentRange { first, last, total })
}

/// Whether an answer is the range that was asked for: bytes [offset, offset + length) of a
/// payload `payload_size` bytes long (the signed size). `content_range` is the response's
/// Content-Range header text, empty when it had none; `body_bytes` the length of the body.
///
/// - 206: Content-Range must be "bytes <offset>-<offset+length-1>/<payload_size>" (the total may
///   be "*"), and the body exactly `length` bytes. An answer that is another part of the file, or
///   of a file of another length, is refused: the payload may have been replaced.
/// - 200: the server ignored `Range` and sent the whole file. Acceptable only when that IS what
///   was asked for (offset 0, the whole payload) and the body is all of it; any other 200 is
///   Unsupported with [`PARTIAL_DOWNLOADS_REFUSED`].
/// - other statuses: FileImportFailure whose context begins `status=<n>` (the convention of the
///   feed's Fetch).
pub fn check_range_response(
    status: i32,
    content_range: &str,
    body_bytes: u64,
    offset: u64,
    length: u64,
    payload_size: u64,
) -> Status {
    let asked = format!("bytes {offset}+{length} of {payload_size}");
    let answered206 = format!("status=206; Content-Range '{content_range}'; {asked}");

    if status == 200 {
        // The server ignored `Range` and answered with the whole file. That is what was asked for
        // only when the request was for the whole file.
        if offset == 0 && length == payload_size && body_bytes == payload_size {
            return Ok(());
        }
        return Err(make_error(
            ErrorCode::Unsupported,
            PARTIAL_DOWNLOADS_REFUSED,
            format!("status=200; {asked}"),
        ));
    }
    if status != 206 {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the download server answered with an unexpected status",
            format!("status={status}; {asked}"),
        ));
    }

    let Some(answered) = parse_content_range(content_range) else {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the download server's Content-Range header is missing or malformed",
            answered206,
        ));
    };
    let end = match offset.checked_add(length) {
        Some(end) if length != 0 => end,
        _ => {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "there is no valid byte range to ask for",
                asked,
            ));
        }
    };
    if answered.first != offset || answered.last != end - 1 {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the download server answered with another part of the file than the one asked for",
            answered206,
        ));
    }
    if answered.total.is_some_and(|total| total != payload_size) {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the download server's file is not the length the release names; it may have been \
             replaced",
            answered206,
        ));
    }
    if body_bytes != length {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            format!("the download server sent {body_bytes} bytes for a range of {length}"),
            format!("status=206; {asked}"),
        ));
    }
    Ok(())
}
