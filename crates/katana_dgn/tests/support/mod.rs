//! What the dgn tests share: V7 designs built byte by byte in the test, from
//! the layout GDAL's dgnlib reads (the offsets each builder names), so that
//! no file of unknown origin is committed and the reader is never tested
//! against the writer alone. The integers and reals are put by
//! katana_dgn::codes's int32_bytes and vax_bytes, which tests/codes.rs holds
//! to hand-worked bytes.

#![allow(
    dead_code,
    reason = "Each test file uses a part of the builders."
)]

use std::path::PathBuf;

use katana_dgn::codes::{int32_bytes, vax_bytes};

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// A VAX real's eight bytes; the test's own values are all within range.
pub fn real(value: f64) -> [u8; 8] {
    vax_bytes(value).expect("a value the format holds")
}

/// The symbology and flags of an element the test builds.
#[derive(Clone, Copy, Default)]
pub struct Look {
    pub colour: u8,
    pub style: u8,
    pub weight: u8,
    pub properties: u16,
    pub complex: bool,
    pub deleted: bool,
}

/// One element: the header word (level, complex bit; type, deleted bit), the
/// words to follow, a range of zeros (the reader does not read it), the
/// display header (dgnread.cpp:1284-1291: graphic group at 28, the
/// attribute index at 30, properties at 32, style and weight at 34, colour
/// at 35), then `data` from byte 36, padded to a word.
pub fn element(kind: u8, level: u8, look: Look, data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 36];
    bytes.extend_from_slice(data);
    if !bytes.len().is_multiple_of(2) {
        bytes.push(0);
    }
    let words = (bytes.len() / 2 - 2) as u16;
    let complex = if look.complex { 0x80 } else { 0 };
    let deleted = if look.deleted { 0x80 } else { 0 };
    bytes[0] = level | complex;
    bytes[1] = kind | deleted;
    bytes[2..4].copy_from_slice(&words.to_le_bytes());
    let attribute_index = ((bytes.len() - 32) / 2) as u16;
    bytes[30..32].copy_from_slice(&attribute_index.to_le_bytes());
    bytes[32..34].copy_from_slice(&look.properties.to_le_bytes());
    bytes[34] = (look.style & 0x07) | (look.weight << 3);
    bytes[35] = look.colour;
    bytes
}

/// A design: its 768-word header (dgnread.cpp:1488-1585) with the working
/// units at 1112 and 1116, their names at 1120 and 1122, the 3D flag at 1214
/// and the global origin at 1240, then the elements added, then FF FF.
pub struct Design {
    pub three_d: bool,
    pub bytes: Vec<u8>,
}

impl Design {
    pub fn new(three_d: bool, subunits: i32, units: i32, origin: [f64; 3]) -> Self {
        let mut header = vec![0_u8; 1536];
        header[0..4].copy_from_slice(if three_d {
            &[0xC8, 0x09, 0xFE, 0x02]
        } else {
            &[0x08, 0x09, 0xFE, 0x02]
        });
        header[1112..1116].copy_from_slice(&int32_bytes(subunits));
        header[1116..1120].copy_from_slice(&int32_bytes(units));
        header[1120..1122].copy_from_slice(b"m\0");
        header[1122..1124].copy_from_slice(b"mm");
        if three_d {
            header[1214] = 0x40;
        }
        for (axis, value) in origin.iter().enumerate() {
            header[1240 + 8 * axis..1248 + 8 * axis].copy_from_slice(&real(*value));
        }
        Self {
            three_d,
            bytes: header,
        }
    }

    /// Metres and millimetres at 10 000 units of resolution a metre, the
    /// global origin at 0,0,0.
    pub fn metric(three_d: bool) -> Self {
        Self::new(three_d, 1000, 10, [0.0; 3])
    }

    pub fn add(&mut self, element: Vec<u8>) -> &mut Self {
        self.bytes.extend_from_slice(&element);
        self
    }

    /// The design's bytes, the end-of-design word last.
    pub fn finish(&self) -> Vec<u8> {
        let mut bytes = self.bytes.clone();
        bytes.extend_from_slice(&[0xFF, 0xFF]);
        bytes
    }

    /// An integer point: two coordinates, or three in a 3D design.
    pub fn point(&self, x: i32, y: i32, z: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&int32_bytes(x));
        bytes.extend_from_slice(&int32_bytes(y));
        if self.three_d {
            bytes.extend_from_slice(&int32_bytes(z));
        }
        bytes
    }

    /// A line (type 3): its two points from byte 36 (dgnread.cpp:435-450).
    pub fn line(&self, level: u8, look: Look, from: (i32, i32, i32), to: (i32, i32, i32)) -> Vec<u8> {
        let mut data = self.point(from.0, from.1, from.2);
        data.extend(self.point(to.0, to.1, to.2));
        element(3, level, look, &data)
    }

    /// A line string (4) or shape (6): the count at 36, the points from 38
    /// (dgnread.cpp:478-551).
    pub fn points(&self, kind: u8, level: u8, look: Look, points: &[(i32, i32, i32)]) -> Vec<u8> {
        let mut data = (points.len() as u16).to_le_bytes().to_vec();
        for point in points {
            data.extend(self.point(point.0, point.1, point.2));
        }
        element(kind, level, look, &data)
    }

    /// An arc (16): the start angle at 36, the sweep at 40 as a sign and a
    /// magnitude, the axes at 44 and 52 (reals), then a 2D rotation at 60 and
    /// the origin at 64, or a 3D quaternion at 60 and the origin at 76
    /// (dgnread.cpp:662-728). Angles in 1/360000 degree, lengths and the
    /// origin in units of resolution.
    #[allow(clippy::too_many_arguments, reason = "an arc element's fields")]
    pub fn arc(
        &self,
        level: u8,
        look: Look,
        start: i32,
        sweep: i32,
        axes: (f64, f64),
        orientation: &[i32],
        origin: (f64, f64, f64),
    ) -> Vec<u8> {
        let mut data = int32_bytes(start).to_vec();
        let mut sweep_bytes = int32_bytes(sweep.abs());
        if sweep < 0 {
            sweep_bytes[1] |= 0x80;
        }
        data.extend_from_slice(&sweep_bytes);
        data.extend_from_slice(&real(axes.0));
        data.extend_from_slice(&real(axes.1));
        for value in orientation {
            data.extend_from_slice(&int32_bytes(*value));
        }
        data.extend_from_slice(&real(origin.0));
        data.extend_from_slice(&real(origin.1));
        if self.three_d {
            data.extend_from_slice(&real(origin.2));
        }
        element(16, level, look, &data)
    }

    /// An ellipse (15): the axes at 36 and 44, then a 2D rotation at 52 and
    /// the origin at 56, or a 3D quaternion at 52 and the origin at 68
    /// (dgnread.cpp:609-660).
    pub fn ellipse(
        &self,
        level: u8,
        look: Look,
        axes: (f64, f64),
        orientation: &[i32],
        origin: (f64, f64, f64),
    ) -> Vec<u8> {
        let mut data = real(axes.0).to_vec();
        data.extend_from_slice(&real(axes.1));
        for value in orientation {
            data.extend_from_slice(&int32_bytes(*value));
        }
        data.extend_from_slice(&real(origin.0));
        data.extend_from_slice(&real(origin.1));
        if self.three_d {
            data.extend_from_slice(&real(origin.2));
        }
        element(15, level, look, &data)
    }

    /// A text (17): font at 36, justification at 37, width and height at 38
    /// and 42 (in 6/1000 of a unit of resolution), then a 2D rotation at 46,
    /// the origin at 50, the count at 58 and the bytes from 60, or a 3D
    /// quaternion at 46, the origin at 62, the count at 74 and the bytes from
    /// 76 (dgnread.cpp:730-805).
    #[allow(clippy::too_many_arguments, reason = "a text element's fields")]
    pub fn text(
        &self,
        level: u8,
        look: Look,
        font: u8,
        justification: u8,
        size: i32,
        orientation: &[i32],
        origin: (i32, i32, i32),
        text: &[u8],
    ) -> Vec<u8> {
        let mut data = vec![font, justification];
        data.extend_from_slice(&int32_bytes(size));
        data.extend_from_slice(&int32_bytes(size));
        for value in orientation {
            data.extend_from_slice(&int32_bytes(*value));
        }
        data.extend(self.point(origin.0, origin.1, origin.2));
        data.push(text.len() as u8);
        data.push(0);
        data.extend_from_slice(text);
        element(17, level, look, &data)
    }

    /// A complex chain (12) or shape (14) header: its total length at 36 -
    /// five words of its own after it, then its members' - and the member
    /// count at 38, eight bytes of null linkage after (dgnwrite.cpp:1575-1628).
    pub fn complex_header(&self, kind: u8, level: u8, look: Look, members: &[Vec<u8>]) -> Vec<u8> {
        let total = 5 + members.iter().map(|member| member.len() / 2).sum::<usize>();
        let mut data = (total as u16).to_le_bytes().to_vec();
        data.extend_from_slice(&(members.len() as u16).to_le_bytes());
        let mut header = element(kind, level, Look { complex: true, ..look }, &data);
        header.extend_from_slice(&[0; 8]);
        // The linkage is the header's attribute data: words and flag again.
        let words = (header.len() / 2 - 2) as u16;
        header[2..4].copy_from_slice(&words.to_le_bytes());
        let properties = look.properties | 0x0800;
        header[32..34].copy_from_slice(&properties.to_le_bytes());
        header
    }

    /// A cell header (2): its total length at 36 - the words after the 38
    /// bytes before it, its own 27 (2D) or 43 (3D) and its components' - and
    /// its name in RADIX-50 at 38 and 40 (dgnread.cpp:268-366,
    /// dgnwrite.cpp:2150).
    pub fn cell_header(&self, level: u8, name: (u16, u16), components: &[Vec<u8>]) -> Vec<u8> {
        let own = if self.three_d { 43 } else { 27 };
        let total = own + components.iter().map(|component| component.len() / 2).sum::<usize>();
        let mut data = vec![0_u8; 2 * own + 2];
        data[0..2].copy_from_slice(&(total as u16).to_le_bytes());
        data[2..4].copy_from_slice(&name.0.to_le_bytes());
        data[4..6].copy_from_slice(&name.1.to_le_bytes());
        element(2, level, Look::default(), &data)
    }

    /// A colour table (group data, level 1): the background's three bytes at
    /// 38, colours 0-254 from 41 (dgnread.cpp:1330-1354).
    pub fn colour_table(&self, table: &[[u8; 3]; 256]) -> Vec<u8> {
        let mut data = vec![0_u8; 2];
        data.extend_from_slice(&table[255]);
        for entry in &table[..255] {
            data.extend_from_slice(entry);
        }
        element(5, 1, Look::default(), &data)
    }
}

/// The complex bit set, for a member of a complex element or a cell.
pub fn member(look: Look) -> Look {
    Look {
        complex: true,
        ..look
    }
}

/// A folder of the test's own, removed afterwards.
pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("katana_dgn_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch folder is made");
        Self { root }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// xorshift64*, a fixed sequence for the damaged copies (Vigna, "An
/// experimental exploration of Marsaglia's xorshift generators", 2016).
pub struct Random(u64);

impl Random {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_value(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_value() % bound as u64) as usize
        }
    }
}
