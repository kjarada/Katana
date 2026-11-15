//! A TIN surface as bytes, and back: how a project keeps its surfaces
//! (docs/terrain.md, "Surfaces in the drawing"). The storage layer may not see
//! the terrain layer (tools/check_layering.cmake), so it keeps each surface as
//! an opaque record whose bytes are written and read here.
//!
//! Exact. Every coordinate is its IEEE-754 bit pattern, so a surface reads back
//! bit for bit - a negative zero stays negative, 0.1 stays the double nearest
//! 0.1 - and the constrained edges (breaklines, a boundary) are kept with the
//! triangles they belong to. The adjacency is not stored: TinSurface::create
//! derives it, which is also what checks the bytes on the way in. It checks the
//! STRUCTURE: the magic, the version, the counts against the length, every
//! index in range, the triangles counter-clockwise, no edge shared by more than
//! two triangles, a coordinate finite. A record damaged in that way is refused
//! with a sentence. It does NOT detect a coordinate changed to another finite
//! value, because the record carries no checksum (docs/terrain.md, "Not done").
//!
//! Version 1, little-endian throughout:
//!
//!   "KTIN"                       4 bytes, the magic
//!   version                      u32, 1
//!   vertex count V, triangle T   u64 each
//!   V vertices                   x, y, z as f64 bit patterns (24 bytes each)
//!   T triangles                  three u32 vertex indices (12 bytes each)
//!   T constraint masks           one byte each, bit k for edge k
//!
//! so a surface takes 24 + 24 V + 13 T bytes. A version is never changed once
//! released; a later layout is a new version read beside this one, and a
//! version newer than this build knows is refused with Unsupported rather than
//! guessed at.

use katana_core::{Error, ErrorCode, Result};

use crate::tin_surface::{Point3, TinSurface, TinTriangle};

/// The version encode_tin writes.
pub const TIN_CODEC_VERSION: u32 = 1;

const MAGIC: &[u8; 4] = b"KTIN";
/// The magic, the version and the two counts.
const HEADER_BYTES: u64 = 4 + 4 + 8 + 8;
const VERTEX_BYTES: u64 = 3 * 8;
/// Three indices and the constraint mask.
const TRIANGLE_BYTES: u64 = 3 * 4 + 1;

fn append_little(out: &mut Vec<u8>, value: u64, bytes: usize) {
    for i in 0..bytes {
        out.push(((value >> (8 * i)) & 0xFF) as u8);
    }
}

/// Reads what append_little wrote. Every read is bounds-checked by the caller,
/// which has compared the whole size with the counts first.
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn little(&mut self, size: usize) -> u64 {
        let mut value = 0u64;
        for i in 0..size {
            value |= u64::from(self.bytes[self.offset + i]) << (8 * i);
        }
        self.offset += size;
        value
    }
    fn real(&mut self) -> f64 {
        f64::from_bits(self.little(8))
    }
    fn byte(&mut self) -> u8 {
        self.little(1) as u8
    }
}

fn damaged(what: &str) -> Error {
    Error::new(
        ErrorCode::ParseFailure,
        format!("a stored surface is damaged: {what}"),
    )
}

/// The bytes encode_tin writes for a surface of `vertices` and `triangles`,
/// without writing them: what a size check before a save needs.
pub fn encoded_tin_bytes(vertices: u64, triangles: u64) -> u64 {
    HEADER_BYTES + VERTEX_BYTES * vertices + TRIANGLE_BYTES * triangles
}

/// `surface` as version-1 bytes. Never fails: every TinSurface is encodable.
pub fn encode_tin(surface: &TinSurface) -> Vec<u8> {
    let vertices = surface.vertices();
    let triangles = surface.triangles();
    let masks = surface.constrained_edges();
    let mut out: Vec<u8> = Vec::with_capacity(encoded_tin_bytes(
        vertices.len() as u64,
        triangles.len() as u64,
    ) as usize);
    out.extend_from_slice(MAGIC);
    append_little(&mut out, u64::from(TIN_CODEC_VERSION), 4);
    append_little(&mut out, vertices.len() as u64, 8);
    append_little(&mut out, triangles.len() as u64, 8);
    for vertex in vertices {
        append_little(&mut out, vertex.x.to_bits(), 8);
        append_little(&mut out, vertex.y.to_bits(), 8);
        append_little(&mut out, vertex.z.to_bits(), 8);
    }
    for triangle in triangles {
        for &index in triangle {
            append_little(&mut out, u64::from(index), 4);
        }
    }
    // TinSurface::create gives every triangle a mask, zero when none was
    // given; the empty surface (no triangles) has none to write.
    out.extend((0..triangles.len()).map(|t| masks.get(t).copied().unwrap_or(0)));
    out
}

/// The surface `bytes` hold. ParseFailure, naming what is wrong, for bytes that
/// are not the form - no magic, a header cut short, counts the bytes cannot
/// hold, bytes left over; Unsupported for a version newer than this build
/// writes; and what TinSurface::create refuses (an index out of range, a
/// clockwise triangle, a non-finite coordinate ...) for a triangulation that
/// is not valid. Nothing is allocated for a count before the bytes are known
/// to hold it.
pub fn decode_tin(bytes: &[u8]) -> Result<TinSurface> {
    if (bytes.len() as u64) < HEADER_BYTES {
        return Err(damaged(&format!(
            "its header is cut short ({} bytes)",
            bytes.len()
        )));
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(damaged(
            "it does not begin KTIN, so it is no surface Katana wrote",
        ));
    }
    let mut reader = Reader {
        bytes: &bytes[MAGIC.len()..],
        offset: 0,
    };
    let version = reader.little(4);
    if version > u64::from(TIN_CODEC_VERSION) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "a stored surface was written by a newer version of Katana",
        )
        .with_context(format!("version={version} supported={TIN_CODEC_VERSION}")));
    }
    if version == 0 {
        return Err(damaged("its version is 0, which no Katana writes"));
    }
    let vertex_count = reader.little(8);
    let triangle_count = reader.little(8);
    // Each count against what the bytes can hold BEFORE they are multiplied
    // or allocated for: a count the record chose must not choose the size of
    // an allocation, nor overflow the sum below.
    let body = bytes.len() as u64 - HEADER_BYTES;
    if vertex_count > body / VERTEX_BYTES || triangle_count > body / TRIANGLE_BYTES {
        return Err(damaged(
            "it counts more vertices or triangles than its bytes hold",
        ));
    }
    let expected = encoded_tin_bytes(vertex_count, triangle_count);
    if expected != bytes.len() as u64 {
        return Err(damaged(&format!(
            "it holds {} bytes where its counts say {expected}",
            bytes.len()
        )));
    }
    let mut vertices: Vec<Point3> = Vec::with_capacity(vertex_count as usize);
    for _ in 0..vertex_count {
        let x = reader.real();
        let y = reader.real();
        let z = reader.real();
        vertices.push(Point3::new(x, y, z));
    }
    let mut triangles: Vec<TinTriangle> = Vec::with_capacity(triangle_count as usize);
    for _ in 0..triangle_count {
        let mut triangle = [0u32; 3];
        for index in &mut triangle {
            *index = reader.little(4) as u32;
        }
        triangles.push(triangle);
    }
    let mut masks: Vec<u8> = Vec::with_capacity(triangle_count as usize);
    for _ in 0..triangle_count {
        masks.push(reader.byte());
    }
    // The adjacency is derived, and every rule a triangulation keeps checked,
    // by the one constructor of a surface from arrays.
    TinSurface::create(vertices, triangles, masks).map_err(|error| {
        Error::new(
            error.code,
            format!(
                "a stored surface is not a valid triangulation: {}",
                error.message
            ),
        )
        .with_context(error.context)
    })
}
