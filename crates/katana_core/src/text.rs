pub const fn is_ascii_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

pub const fn ascii_lower(byte: u8) -> u8 {
    byte.to_ascii_lowercase()
}

pub const fn ascii_upper(byte: u8) -> u8 {
    byte.to_ascii_uppercase()
}

pub fn trimmed(mut text: &[u8]) -> &[u8] {
    while text.first().is_some_and(|byte| is_ascii_space(*byte)) {
        text = &text[1..];
    }
    while text.last().is_some_and(|byte| is_ascii_space(*byte)) {
        text = &text[..text.len() - 1];
    }
    text
}

pub fn lowered(text: impl AsRef<[u8]>) -> Vec<u8> {
    text.as_ref()
        .iter()
        .map(|byte| ascii_lower(*byte))
        .collect()
}

pub fn uppered(text: impl AsRef<[u8]>) -> Vec<u8> {
    text.as_ref()
        .iter()
        .map(|byte| ascii_upper(*byte))
        .collect()
}

pub fn equals_ignoring_case(left: &[u8], right: &[u8]) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn without_plus(token: &[u8]) -> Option<&[u8]> {
    let digits = token.strip_prefix(b"+").unwrap_or(token);
    if digits.is_empty() || (token.starts_with(b"+") && matches!(digits.first(), Some(b'+' | b'-')))
    {
        return None;
    }
    Some(digits)
}

pub fn parse_integer(token: impl AsRef<[u8]>) -> Option<i64> {
    let digits = without_plus(token.as_ref())?;
    std::str::from_utf8(digits).ok()?.parse().ok()
}

pub fn parse_finite_double(token: impl AsRef<[u8]>) -> Option<f64> {
    let digits = without_plus(token.as_ref())?;
    let mut at = usize::from(digits.first() == Some(&b'-'));
    let mut digit_count = 0;
    let mut nonzero = false;
    while at < digits.len() && digits[at].is_ascii_digit() {
        nonzero |= digits[at] != b'0';
        digit_count += 1;
        at += 1;
    }
    if digits.get(at) == Some(&b'.') {
        at += 1;
        while at < digits.len() && digits[at].is_ascii_digit() {
            nonzero |= digits[at] != b'0';
            digit_count += 1;
            at += 1;
        }
    }
    if digit_count == 0 {
        return None;
    }
    if matches!(digits.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(digits.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent_start = at;
        while at < digits.len() && digits[at].is_ascii_digit() {
            at += 1;
        }
        if at == exponent_start {
            return None;
        }
    }
    if at != digits.len() {
        return None;
    }
    let value: f64 = std::str::from_utf8(digits).ok()?.parse().ok()?;
    if !value.is_finite() || (value == 0.0 && nonzero) {
        return None;
    }
    Some(value)
}

pub fn split_lines(text: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < text.len() {
        let byte = text[at];
        if matches!(byte, b'\n' | b'\r') {
            lines.push(&text[start..at]);
            at += 1;
            if byte == b'\r' && text.get(at) == Some(&b'\n') {
                at += 1;
            }
            start = at;
        } else {
            at += 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn non_finite_text(value: f64) -> Option<String> {
    if value.is_finite() {
        return None;
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    Some(format!(
        "{sign}{}",
        if value.is_nan() { "nan" } else { "inf" }
    ))
}

fn scientific_parts(text: &str) -> (&str, i32) {
    let (mantissa, exponent) = text
        .split_once('e')
        .expect("scientific format has an exponent");
    (mantissa, exponent.parse().expect("float exponent fits i32"))
}

fn scientific_text(mantissa: &str, exponent: i32) -> String {
    format!("{mantissa}e{exponent:+03}")
}

fn shortest_ties_to_even(value: f64, mantissa: &str, exponent: i32) -> String {
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let decimal: u64 = digits
        .parse()
        .expect("a double has at most 17 significant digits");
    if decimal.is_multiple_of(2) {
        return mantissa.into();
    }
    let bits = value.abs().to_bits();
    let binary_exponent = (bits >> 52) as i32;
    let binary = (bits & ((1_u64 << 52) - 1)) | if binary_exponent == 0 { 0 } else { 1_u64 << 52 };
    let trailing = binary.trailing_zeros();
    let power = if binary_exponent == 0 {
        -1074
    } else {
        binary_exponent - 1075
    };
    let decimal_power = exponent - (digits.len() as i32 - 1);
    if power + trailing as i32 != decimal_power - 1 {
        return mantissa.into();
    }
    let mut midpoint = decimal * 2 - 1;
    if decimal_power >= 0 {
        for _ in 0..decimal_power {
            let Some(product) = midpoint.checked_mul(5) else {
                return mantissa.into();
            };
            midpoint = product;
        }
    } else {
        for _ in decimal_power..0 {
            if !midpoint.is_multiple_of(5) {
                return mantissa.into();
            }
            midpoint /= 5;
        }
    }
    if midpoint != binary >> trailing {
        return mantissa.into();
    }
    let mut adjusted = mantissa.to_owned();
    let last = adjusted.pop().expect("a mantissa has a final digit");
    adjusted.push(char::from(last as u8 - 1));
    if scientific_text(&adjusted, exponent)
        .parse::<f64>()
        .is_ok_and(|parsed| parsed.to_bits() == value.to_bits())
    {
        adjusted
    } else {
        mantissa.into()
    }
}

fn fixed_mantissa(mantissa: &str, exponent: i32) -> String {
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(unsigned) => ("-", unsigned),
        None => ("", mantissa),
    };
    let digits = mantissa.replace('.', "");
    let decimal_at = exponent + 1;
    if decimal_at <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-decimal_at) as usize))
    } else if decimal_at as usize >= digits.len() {
        format!(
            "{sign}{digits}{}",
            "0".repeat(decimal_at as usize - digits.len())
        )
    } else {
        let (whole, fractional) = digits.split_at(decimal_at as usize);
        format!("{sign}{whole}.{fractional}")
    }
}

pub fn format_exact_real(value: f64) -> String {
    if let Some(text) = non_finite_text(value) {
        return text;
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific_parts(&scientific);
    let mantissa = shortest_ties_to_even(value, mantissa, exponent);
    let scientific = scientific_text(&mantissa, exponent);
    let fixed = if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        fixed_mantissa(&mantissa, exponent)
    };
    if fixed.len() <= scientific.len() {
        fixed
    } else {
        scientific
    }
}

pub fn format_fixed(value: f64, decimals: i32) -> String {
    if let Some(text) = non_finite_text(value) {
        return text;
    }
    let decimals = decimals.clamp(0, 60) as usize;
    let mut text = format!("{value:.decimals$}");
    if text.starts_with('-') && text.bytes().all(|byte| matches!(byte, b'-' | b'0' | b'.')) {
        text.remove(0);
    }
    text
}

pub fn format_general(value: f64, significant_digits: usize) -> String {
    if let Some(text) = non_finite_text(value) {
        return text;
    }
    let significant_digits = significant_digits.max(1);
    let scientific = format!("{:.*e}", significant_digits - 1, value);
    let (mantissa, exponent) = scientific_parts(&scientific);
    let mantissa = if mantissa.contains('.') {
        mantissa.trim_end_matches('0').trim_end_matches('.')
    } else {
        mantissa
    };
    if exponent < -4 || i64::from(exponent) >= significant_digits as i64 {
        return scientific_text(mantissa, exponent);
    }
    fixed_mantissa(mantissa, exponent)
}

pub fn unescape_typed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.peek() == Some(&'n') {
            out.push('\n');
            characters.next();
        } else if character == '\\' && characters.peek() == Some(&'\\') {
            out.push('\\');
            characters.next();
        } else {
            out.push(character);
        }
    }
    out
}

pub fn reply_quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '\n' => out.push_str("\\n"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(character),
        }
    }
    out.push('"');
    out
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReplyRecord {
    pub words: Vec<String>,
    pub fields: Vec<(String, String)>,
}

impl ReplyRecord {
    pub fn value(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, text)| text.as_str())
    }
}

pub fn read_reply_record(line: &str) -> Option<ReplyRecord> {
    let bytes = line.as_bytes();
    let mut record = ReplyRecord::default();
    let mut at = 0;
    while at < bytes.len() {
        if is_ascii_space(bytes[at]) {
            at += 1;
            continue;
        }
        let mut end = at;
        while end < bytes.len() && !is_ascii_space(bytes[end]) && bytes[end] != b'=' {
            end += 1;
        }
        if end == bytes.len() || bytes[end] != b'=' {
            if !record.fields.is_empty() {
                return None;
            }
            record.words.push(line[at..end].into());
            at = end;
            continue;
        }
        if end == at {
            return None;
        }
        let key = line[at..end].into();
        at = end + 1;
        let mut value = Vec::new();
        if bytes.get(at) == Some(&b'"') {
            at += 1;
            let mut closed = false;
            while at < bytes.len() {
                let byte = bytes[at];
                at += 1;
                if byte == b'"' {
                    closed = true;
                    break;
                }
                if byte == b'\\' && at < bytes.len() {
                    let next = bytes[at];
                    at += 1;
                    value.push(if next == b'n' { b'\n' } else { next });
                } else {
                    value.push(byte);
                }
            }
            if !closed {
                return None;
            }
        } else {
            while at < bytes.len() && !is_ascii_space(bytes[at]) {
                value.push(bytes[at]);
                at += 1;
            }
        }
        record.fields.push((key, String::from_utf8(value).ok()?));
    }
    if record.words.is_empty() && record.fields.is_empty() {
        None
    } else {
        Some(record)
    }
}
