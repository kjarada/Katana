use crate::{Error, ErrorCode, Result};
use std::fmt;

#[doc = "Unicode Consortium CP1252.TXT mapping; the five undefined bytes retain their C1 control values, as in the C++ oracle."]
const WINDOWS_1252_HIGH: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
];

pub fn is_valid_utf8(text: impl AsRef<[u8]>) -> bool {
    crate::text_kernels::valid_utf8(text.as_ref())
}

#[allow(
    unsafe_code,
    reason = "The selected validator establishes UTF-8 validity of this immutable slice exactly once."
)]
fn validated_utf8(bytes: &[u8]) -> std::result::Result<&str, ()> {
    if is_valid_utf8(bytes) {
        Ok(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        Err(())
    }
}

fn from_windows1252(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() + bytes.len() / 8);
    for byte in bytes {
        let point = if (0x80..0xa0).contains(byte) {
            u32::from(WINDOWS_1252_HIGH[usize::from(*byte - 0x80)])
        } else {
            u32::from(*byte)
        };
        text.push(char::from_u32(point).expect("CP1252 maps to Unicode scalar values"));
    }
    text
}

pub fn utf8_or_windows1252(text: impl AsRef<[u8]>) -> String {
    let bytes = text.as_ref();
    match validated_utf8(bytes) {
        Ok(text) => text.into(),
        Err(_) => from_windows1252(bytes),
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf8WithBom,
    Utf16LittleEndian,
    Utf16BigEndian,
    Windows1252,
}

pub const fn to_string(encoding: TextEncoding) -> &'static str {
    match encoding {
        TextEncoding::Utf8 => "UTF-8",
        TextEncoding::Utf8WithBom => "UTF-8 with a byte order mark",
        TextEncoding::Utf16LittleEndian => "UTF-16 little-endian",
        TextEncoding::Utf16BigEndian => "UTF-16 big-endian",
        TextEncoding::Windows1252 => "Windows-1252",
    }
}

impl fmt::Display for TextEncoding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(to_string(*self))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DecodedText {
    pub text: String,
    pub encoding: TextEncoding,
    pub guessed: bool,
    pub validated_utf8: bool,
}

fn from_utf16(bytes: &[u8], little_endian: bool) -> Result<String> {
    if !bytes.len().is_multiple_of(2) {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "the file is UTF-16 but has an odd number of bytes, so it is truncated",
        ));
    }
    let unit = |index: usize| {
        let pair = [bytes[2 * index], bytes[2 * index + 1]];
        u32::from(if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        })
    };
    let units = bytes.len() / 2;
    let mut out = String::with_capacity(units + 16);
    let mut index = 0;
    while index < units {
        index +=
            crate::text_kernels::append_ascii_utf16(&bytes[2 * index..], little_endian, &mut out);
        if index == units {
            break;
        }
        let mut code_point = unit(index);
        if (0xd800..=0xdbff).contains(&code_point) {
            if index + 1 >= units {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    "the file ends in the middle of a UTF-16 surrogate pair",
                ));
            }
            let low = unit(index + 1);
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    "malformed UTF-16: a high surrogate with no low surrogate",
                )
                .with_context(format!("byte {}", 2 * index)));
            }
            code_point = 0x10000 + ((code_point - 0xd800) << 10) + (low - 0xdc00);
            index += 1;
        } else if (0xdc00..=0xdfff).contains(&code_point) {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "malformed UTF-16: a low surrogate with no high surrogate",
            )
            .with_context(format!("byte {}", 2 * index)));
        }
        out.push(char::from_u32(code_point).expect("validated UTF-16 scalar"));
        index += 1;
    }
    Ok(out)
}

fn validated(text: String, encoding: TextEncoding, guessed: bool) -> DecodedText {
    DecodedText {
        text,
        encoding,
        guessed,
        validated_utf8: true,
    }
}

pub fn decode_text(bytes: impl AsRef<[u8]>) -> Result<DecodedText> {
    let bytes = bytes.as_ref();
    if let Some(bytes) = bytes.strip_prefix(b"\xff\xfe") {
        return Ok(validated(
            from_utf16(bytes, true)?,
            TextEncoding::Utf16LittleEndian,
            false,
        ));
    }
    if let Some(bytes) = bytes.strip_prefix(b"\xfe\xff") {
        return Ok(validated(
            from_utf16(bytes, false)?,
            TextEncoding::Utf16BigEndian,
            false,
        ));
    }
    if let Some(bytes) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        let text = validated_utf8(bytes).map_err(|_| {
            Error::new(
                ErrorCode::ParseFailure,
                "the file is marked as UTF-8 but contains bytes that are not",
            )
        })?;
        return Ok(validated(text.into(), TextEncoding::Utf8WithBom, false));
    }
    let sample = bytes.len().min(4096);
    let mut nuls_even = 0;
    let mut nuls_odd = 0;
    for (index, byte) in bytes[..sample].iter().enumerate() {
        if *byte == 0 {
            if index.is_multiple_of(2) {
                nuls_even += 1;
            } else {
                nuls_odd += 1;
            }
        }
    }
    if sample >= 8 && nuls_odd > sample / 4 && nuls_odd > nuls_even {
        return Ok(validated(
            from_utf16(bytes, true)?,
            TextEncoding::Utf16LittleEndian,
            true,
        ));
    }
    if sample >= 8 && nuls_even > sample / 4 {
        return Ok(validated(
            from_utf16(bytes, false)?,
            TextEncoding::Utf16BigEndian,
            true,
        ));
    }
    match validated_utf8(bytes) {
        Ok(text) => Ok(validated(text.into(), TextEncoding::Utf8, false)),
        Err(_) => Ok(validated(
            from_windows1252(bytes),
            TextEncoding::Windows1252,
            true,
        )),
    }
}

pub fn decode_text_as(bytes: impl AsRef<[u8]>, encoding: TextEncoding) -> Result<DecodedText> {
    let bytes = bytes.as_ref();
    let mark_little = bytes.starts_with(b"\xff\xfe");
    let mark_big = bytes.starts_with(b"\xfe\xff");
    let mark_utf8 = bytes.starts_with(b"\xef\xbb\xbf");
    let contradicted =
        |found: &str| {
            Error::new(ErrorCode::ParseFailure,
        format!("the file begins with a {found} byte order mark but was declared to be {encoding}"))
        .with_context("declared and marked encodings disagree")
        };
    match encoding {
        TextEncoding::Utf16LittleEndian | TextEncoding::Utf16BigEndian => {
            let little = encoding == TextEncoding::Utf16LittleEndian;
            if (little && mark_big) || (!little && mark_little) {
                return Err(contradicted(if little {
                    "UTF-16 big-endian"
                } else {
                    "UTF-16 little-endian"
                }));
            }
            if mark_utf8 {
                return Err(contradicted("UTF-8"));
            }
            let marked = if little { mark_little } else { mark_big };
            let text = from_utf16(if marked { &bytes[2..] } else { bytes }, little)?;
            Ok(validated(text, encoding, false))
        }
        TextEncoding::Utf8 | TextEncoding::Utf8WithBom => {
            if mark_little || mark_big {
                return Err(contradicted(if mark_little {
                    "UTF-16 little-endian"
                } else {
                    "UTF-16 big-endian"
                }));
            }
            let text =
                validated_utf8(if mark_utf8 { &bytes[3..] } else { bytes }).map_err(|_| {
                    Error::new(
                        ErrorCode::ParseFailure,
                        "the file was declared to be UTF-8 but contains bytes that are not",
                    )
                })?;
            Ok(validated(
                text.into(),
                if mark_utf8 {
                    TextEncoding::Utf8WithBom
                } else {
                    TextEncoding::Utf8
                },
                false,
            ))
        }
        TextEncoding::Windows1252 => {
            if mark_little || mark_big || mark_utf8 {
                return Err(contradicted(if mark_utf8 {
                    "UTF-8"
                } else if mark_little {
                    "UTF-16 little-endian"
                } else {
                    "UTF-16 big-endian"
                }));
            }
            Ok(validated(from_windows1252(bytes), encoding, false))
        }
    }
}

pub fn encode_utf16_little_endian(utf8: impl AsRef<[u8]>) -> Result<Vec<u8>> {
    let utf8 = validated_utf8(utf8.as_ref())
        .map_err(|_| Error::new(ErrorCode::InvalidArgument, "the text is not valid UTF-8"))?;
    let mut out = Vec::with_capacity(utf8.len() * 2 + 2);
    out.extend_from_slice(b"\xff\xfe");
    for unit in utf8.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(out)
}
