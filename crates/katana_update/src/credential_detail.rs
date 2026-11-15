//! What the two credential sources (credential.rs, credential_win.rs) share between themselves,
//! and the seams of the Windows store that a test needs. Internal to the update layer and its
//! tests, as the C++'s private header is: the public contract is the credential module.

use katana_core::error::make_error;
use katana_core::text::trimmed;
use katana_core::text_encoding::is_valid_utf8;
use katana_core::{Error, ErrorCode, Result};

#[cfg(windows)]
pub use crate::credential_win::{
    credential_error, credential_target_of, make_credential_manager_store,
    make_credential_manager_store_failing_with,
};

/// The most bytes a token may have. The Windows Credential Manager refuses a blob over
/// 5 * 512 = 2560 bytes (CRED_MAX_CREDENTIAL_BLOB_SIZE in the Windows SDK's wincred.h;
/// mingw-w64's copy says 512, which is not what the system enforces - measured on Windows 11 on
/// 2026-10-08: 2560 accepted, 2561 refused with error 1783), and a token the store could not hold
/// is not one to accept from the environment either. GitHub advises storing its tokens in 255
/// characters (its 2021 note on the new token formats), far inside this.
pub const MAX_TOKEN_BYTES: usize = 2560;

// Whether `text` starts with a code point that is white space or invisible and no part of a
// token, and that katana_core's trimmed (ASCII only) does not know: the length of its UTF-8 form
// in bytes (2 or 3), else 0. A token copied from a web page, a chat or a PDF carries these at its
// ends (a no-break space, a zero-width space, a direction mark), and an editor that saves "UTF-8"
// can put a byte-order mark in front of one in a file; the server answers a 401 that names no
// cause. The code points are Unicode's White_Space characters and the zero-width and direction
// format characters; each is matched on its encoded bytes, worked out from the code point:
//   U+0085 C2 85                U+00A0 C2 A0
//   U+1680 E1 9A 80             U+180E E1 A0 8E
//   U+2000..U+200F E2 80 80..8F (spaces, zero-width characters, direction marks)
//   U+2028..U+202F E2 80 A8..AF (line and paragraph separators, direction controls, narrow space)
//   U+205F..U+206F E2 81 9F..AF (medium space, word joiner, invisible operators, direction
//                                isolates, the deprecated format controls)
//   U+3000 E3 80 80             U+FEFF EF BB BF
fn invisible_length_at_front(text: &[u8]) -> usize {
    let byte_at = |index: usize| text.get(index).copied().unwrap_or(0);
    let first = byte_at(0);
    let second = byte_at(1);
    let third = byte_at(2);
    let three_if = |matched: bool| if matched { 3 } else { 0 };
    match first {
        0xc2 => {
            if second == 0x85 || second == 0xa0 {
                2
            } else {
                0
            }
        }
        0xe1 => three_if((second == 0x9a && third == 0x80) || (second == 0xa0 && third == 0x8e)),
        0xe2 => match second {
            0x80 => three_if((0x80..=0x8f).contains(&third) || (0xa8..=0xaf).contains(&third)),
            0x81 => three_if((0x9f..=0xaf).contains(&third)),
            _ => 0,
        },
        0xe3 => three_if(second == 0x80 && third == 0x80),
        0xef => three_if(second == 0xbb && third == 0xbf),
        _ => 0,
    }
}

// The same for the end of `text`. Every lead byte above is a lead byte, never the continuation of
// a longer character (those are 0x80-0xBF), so the end of a well-formed text cannot be mistaken.
fn invisible_length_at_back(text: &[u8]) -> usize {
    for length in [2, 3] {
        if text.len() >= length && invisible_length_at_front(&text[text.len() - length..]) == length
        {
            return length;
        }
    }
    0
}

// `token` without the ASCII blanks (trimmed) and the invisible code points around it, in any
// order and number.
fn without_blanks(mut token: &[u8]) -> &[u8] {
    loop {
        let mut changed = false;
        let ascii = trimmed(token);
        if ascii.len() != token.len() {
            token = ascii;
            changed = true;
        }
        let front = invisible_length_at_front(token);
        if front != 0 {
            token = &token[front..];
            changed = true;
        }
        let back = invisible_length_at_back(token);
        if back != 0 {
            token = &token[..token.len() - back];
            changed = true;
        }
        if !changed {
            return token;
        }
    }
}

fn refusal(message: impl Into<String>, source: &str) -> Error {
    make_error(ErrorCode::InvalidArgument, message, source)
}

/// `token` itself, without the white space and the invisible characters around it (ASCII blanks,
/// and Unicode's White_Space and zero-width and direction format characters, in any order and
/// number: a token copied from a web page or a chat arrives with a no-break space, and an editor
/// that saves "UTF-8" can put a byte-order mark in front of one), when it can be used as a token,
/// else why not (InvalidArgument; the callers re-code it InvalidState when what they read is not
/// usable). The callers trim ASCII first to tell "nothing there" from "not usable"; a token
/// pasted into a field or exported from `$(cat file)` arrives with a newline, and none of those
/// characters can be part of one. A token goes into an HTTP header
/// (`Authorization: Bearer <token>`), so a control character - 0x00 to 0x1F and 0x7F, above all a
/// carriage return or a line feed, which would end the header and begin another - is refused
/// wherever the token comes from. So is a space inside it, ASCII or not (a no-break space, a
/// zero-width space): a token is one word, and a person who pastes "Bearer ghp_..." would
/// otherwise send "Authorization: Bearer Bearer ghp_..." and meet a 401 that says nothing of the
/// cause. Other non-ASCII text is allowed: the store keeps whatever valid UTF-8 it is given, and
/// the server is the judge of what it accepts. `source` goes into the error's context and must
/// not be the token.
pub fn usable_token<'a>(token: &'a [u8], source: &str) -> Result<&'a str> {
    let token = without_blanks(token);
    if token.is_empty() {
        return Err(refusal("the token is empty", source));
    }
    if token.len() > MAX_TOKEN_BYTES {
        return Err(refusal(
            format!(
                "the token is too long: {} bytes, and at most {MAX_TOKEN_BYTES} can be kept",
                token.len()
            ),
            source,
        ));
    }
    if token.iter().any(|&byte| byte < 0x20 || byte == 0x7f) {
        return Err(refusal(
            "the token holds a control character, which an HTTP header cannot carry",
            source,
        ));
    }
    if token.contains(&b' ') {
        return Err(refusal(
            "the token holds a space: give the token alone, without \"Bearer\" or any other word \
             before it",
            source,
        ));
    }
    if (0..token.len()).any(|at| invisible_length_at_front(&token[at..]) != 0) {
        return Err(refusal(
            "the token holds a no-break or zero-width space or another invisible character: copy \
             the token again, as plain text",
            source,
        ));
    }
    let not_utf8 = || refusal("the token is not valid UTF-8 text", source);
    if !is_valid_utf8(token) {
        return Err(not_utf8());
    }
    std::str::from_utf8(token).map_err(|_| not_utf8())
}
