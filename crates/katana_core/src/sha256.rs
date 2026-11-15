use crate::path_text::path_to_utf8;
use crate::text_encoding::utf8_or_windows1252;
use crate::{Error, ErrorCode, Result};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

#[doc = "FIPS 180-4 section 5.3.3: the initial SHA-256 state, unchanged from the C++ oracle."]
const INITIAL: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];
#[doc = "FIPS 180-4 section 4.2.2: the SHA-256 round constants, unchanged from the C++ oracle."]
const ROUND: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub type Sha256Digest = [u8; 32];

#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    length: u64,
    used: usize,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self {
            state: INITIAL,
            block: [0; 64],
            length: 0,
            used: 0,
        }
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn update(&mut self, bytes: impl AsRef<[u8]>) {
        let mut bytes = bytes.as_ref();
        if bytes.is_empty() {
            return;
        }
        self.length = self.length.wrapping_add(bytes.len() as u64);
        if self.used > 0 {
            let take = bytes.len().min(64 - self.used);
            self.block[self.used..self.used + take].copy_from_slice(&bytes[..take]);
            self.used += take;
            bytes = &bytes[take..];
            if self.used < 64 {
                return;
            }
            compress(&mut self.state, &self.block);
            self.used = 0;
        }
        let (blocks, remainder) = bytes.as_chunks::<64>();
        for block in blocks {
            compress(&mut self.state, block);
        }
        if !remainder.is_empty() {
            self.block[..remainder.len()].copy_from_slice(remainder);
            self.used = remainder.len();
        }
    }

    pub fn finish(&mut self) -> Sha256Digest {
        let bit_length = self.length.wrapping_mul(8);
        self.block[self.used] = 0x80;
        self.used += 1;
        if self.used > 56 {
            self.block[self.used..].fill(0);
            compress(&mut self.state, &self.block);
            self.used = 0;
        }
        self.block[self.used..56].fill(0);
        self.block[56..].copy_from_slice(&bit_length.to_be_bytes());
        compress(&mut self.state, &self.block);
        let mut digest = [0u8; 32];
        for (bytes, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            bytes.copy_from_slice(&word.to_be_bytes());
        }
        self.reset();
        digest
    }

    pub fn finish_hex(&mut self) -> String {
        to_hex(&self.finish())
    }
}

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut words = [0u32; 64];
    for (word, bytes) in words[..16].iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*bytes);
    }
    for index in 16..64 {
        let sigma0 = words[index - 15].rotate_right(7)
            ^ words[index - 15].rotate_right(18)
            ^ (words[index - 15] >> 3);
        let sigma1 = words[index - 2].rotate_right(17)
            ^ words[index - 2].rotate_right(19)
            ^ (words[index - 2] >> 10);
        words[index] = words[index - 16]
            .wrapping_add(sigma0)
            .wrapping_add(words[index - 7])
            .wrapping_add(sigma1);
    }
    let [
        mut state_a,
        mut state_b,
        mut state_c,
        mut state_d,
        mut state_e,
        mut state_f,
        mut state_g,
        mut state_h,
    ] = *state;
    for (word, round) in words.into_iter().zip(ROUND) {
        let sigma1 = state_e.rotate_right(6) ^ state_e.rotate_right(11) ^ state_e.rotate_right(25);
        let choice = (state_e & state_f) ^ (!state_e & state_g);
        let temp1 = state_h
            .wrapping_add(sigma1)
            .wrapping_add(choice)
            .wrapping_add(round)
            .wrapping_add(word);
        let sigma0 = state_a.rotate_right(2) ^ state_a.rotate_right(13) ^ state_a.rotate_right(22);
        let majority = (state_a & state_b) ^ (state_a & state_c) ^ (state_b & state_c);
        let temp2 = sigma0.wrapping_add(majority);
        state_h = state_g;
        state_g = state_f;
        state_f = state_e;
        state_e = state_d.wrapping_add(temp1);
        state_d = state_c;
        state_c = state_b;
        state_b = state_a;
        state_a = temp1.wrapping_add(temp2);
    }
    for (word, work) in state.iter_mut().zip([
        state_a, state_b, state_c, state_d, state_e, state_f, state_g, state_h,
    ]) {
        *word = word.wrapping_add(work);
    }
}

fn to_hex(digest: &Sha256Digest) -> String {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(HEX_DIGITS[usize::from(byte >> 4)] as char);
        text.push(HEX_DIGITS[usize::from(byte & 0x0f)] as char);
    }
    text
}

pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.finish_hex()
}

pub fn sha256_file_hex(path: &Path) -> Result<String> {
    let error = |code, message| {
        Error::new(code, message).with_context(utf8_or_windows1252(path_to_utf8(path)))
    };
    let not_regular = || {
        error(
            ErrorCode::NotFound,
            "the file cannot be opened: it is not a regular file (a directory, a device or a pipe)",
        )
    };
    if std::fs::metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
        return Err(not_regular());
    }
    let mut file =
        File::open(path).map_err(|_| error(ErrorCode::NotFound, "the file cannot be opened"))?;
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(not_regular());
    }
    let mut hash = Sha256::new();
    let mut chunk = vec![0u8; 256 * 1024];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(got) => hash.update(&chunk[..got]),
            Err(failure) if failure.kind() == ErrorKind::Interrupted => continue,
            Err(_) => {
                return Err(error(
                    ErrorCode::FileImportFailure,
                    "the file cannot be read",
                ));
            }
        }
    }
    Ok(hash.finish_hex())
}

pub fn parse_sha256_hex(hex: impl AsRef<[u8]>) -> Result<Sha256Digest> {
    let hex = hex.as_ref();
    if hex.len() != 64 {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            format!(
                "a SHA-256 digest is 64 hexadecimal characters, not {}",
                hex.len()
            ),
        ));
    }
    let nibble = |byte| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    };
    let mut digest = [0u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        let bad = |offset| {
            Error::new(
                ErrorCode::ParseFailure,
                "a SHA-256 digest is written with the characters 0-9 and a-f only",
            )
            .with_context(format!("character {offset}"))
        };
        let high = nibble(hex[2 * index]).ok_or_else(|| bad(2 * index))?;
        let low = nibble(hex[2 * index + 1]).ok_or_else(|| bad(2 * index + 1))?;
        *byte = (high << 4) | low;
    }
    Ok(digest)
}
