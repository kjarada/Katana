//! The meshes in plan as their faces (docs/plan_view.md, "Meshes in plan"; docs/cad.md, "A mesh is
//! not a surface"): what the plan painter draws of the window's meshes while the window's switch
//! says Faces (cad's MeshPlanDisplay), beneath the surfaces and the drawing.
//!
//! A design model brought in from an archive can be nothing but meshes - kerbs, pavements,
//! batters, 646 meshes of 877 368 triangles in the one that showed it - and drawn as footprints
//! (the dashed hulls the plan drew until 2026-11-19) such a model was an empty plan. Here each
//! face is drawn seen from above: filled in its colour (the face's own where the file gives one),
//! shaded by the 3D views' hillshade of its normal so kerbs, batters and crowns read, the highest
//! face winning where faces overlap (a plan looks down, so a closed kerb shows its top and not its
//! underside), and the rim of an open mesh (the edges one face uses) traced a shade darker once
//! the mesh is large enough on screen for the line not to be the whole of it.
//!
//! HOW. Every face of every mesh in view is scan-converted into one image, a pixel per device
//! pixel on screen and at the imagery's capped resolution on paper - laid out as a surface's
//! faces' image is (cad's plan_image_grid), so meshes and surfaces line up - and the image is
//! KEPT in the cache with what it was made from: the grid (the piece of the model it covers, at
//! what size of pixel), the meshes (by address, size and colours) and the ink. A paint that
//! changes none of them - a selection, a command that touches no mesh, a pan by whole pixels that
//! keeps every mesh wholly in view - lays the kept image down where the view now puts it, and
//! only a zoom or a pan that shows more or less of the meshes makes it again. The faces are binned into bands of rows and the bands
//! rasterised across the task pool; each band takes its faces in the meshes' order and its rows
//! are its own, so the image is the same at any thread count (`TaskPool`, CLAUDE.md section 5).
//!
//! Level of detail, without dropping anything: a pixel is lit when its centre is inside a face
//! (the graphics APIs' rule), and a face too thin to hold the centre of any pixel in a row it
//! crosses lights the pixel nearest its middle there, so a kerb narrower than a pixel at the
//! extents of a site is still a line and a face smaller than a pixel is still a dot. Everything
//! about a mesh that does not depend on the view - its plan box, its faces' shades, its rim - is
//! worked out once, when the mesh is first drawn, and kept while it is drawn.
//!
//! Rejected: stroking or filling each face through the canvas (a path per face: seconds a frame at
//! this size); drawing faces in file order without a depth test (the underside or the far wall of
//! a closed mesh, drawn last, covered its top); the convex hull filled (a horseshoe's hull covers
//! the ground inside it, and a model is many such shapes).

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "!(a > b) rejects NaN, which a <= b would let through: a non-finite vertex is skipped."
)]

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use katana_cad::plot::{PlotColourMode, PlotSettings, luminance, paper_colour};
use katana_cad::scene::{SceneMesh, SceneOptions, hillshade};
use katana_cad::terrain::surface_plan::{SurfacePlanImageGrid, plan_image_grid};
use katana_cad::view_transform::ViewTransform;
use katana_core::task_pool::TaskPool;
use katana_entity::Color as EntityColor;
use katana_geometry::mesh::MeshEdge;
use katana_geometry::{Box2, Point2, TriangleMesh};
use katana_math::Vec3;
use katana_render::{Rgba, blue_of, green_of, red_of};

use crate::canvas::{Canvas, Image, ImageFormat, PointF, RenderHint, Transform};

/// A level face's share of its colour. Below one, so the faces sit under the drawing - whose
/// lines are the brightest thing in the view - and so a face turned to the sun has room to be
/// lighter than a level one: the hillshade lights a level face at 0.866 (SceneOptions' light, a
/// sun 45 degrees up in the north-west: 0.3 of sky and 0.8 x cos 45 of sun) and one facing the sun
/// at 1, so with this share a level face is 0.65 of its colour and the brightest 0.75.
pub const MESH_LEVEL_SHADE: f64 = 0.65;
/// The darkest a face is drawn, as a share of its colour: a 1 in 1 batter facing away from the sun
/// would be 0.2 and a steeper wall darker still, which on the view's dark ground reads as a hole.
pub const MESH_SHADE_FLOOR: f64 = 0.25;
/// A mesh's rim, as a share of the colour of the face it bounds.
pub const MESH_RIM_SHADE: f64 = 0.45;
/// A rim is traced only round a mesh at least this many image pixels across: smaller, the rim
/// would be most of the mesh.
pub const MESH_RIM_MINIMUM_PIXELS: f64 = 24.0;
/// The faces' opacity on paper: the drawing printed over them, black on the faces' colours, must
/// still be read, and a face is context. On screen the faces are opaque over a dark ground.
pub const MESH_FACE_PAPER_OPACITY: f64 = 0.5;

/// Rows a band of the rasterisation holds: the surface faces' bands' size.
const BAND_ROWS: i32 = 32;

/// How a face's colour becomes ink: on screen as it is; on paper with the plot's white rule
/// (white prints black, as a mesh's footprint did) and, in a greyscale or monochrome plot, as the
/// grey of its luminance - an image's rule (plot_image), not a fill's, since a monochrome fill
/// thresholded to black would print the faces as a black slab under the drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MeshInk {
    pub white_to_black: bool,
    pub grey: bool,
}

impl MeshInk {
    pub const SCREEN: Self = Self {
        white_to_black: false,
        grey: false,
    };

    pub fn paper(plot: &PlotSettings) -> Self {
        Self {
            white_to_black: plot.white_to_black,
            grey: plot.colour_mode != PlotColourMode::Colour,
        }
    }

    /// `colour` at `shade` of itself, as opaque RGBA bytes.
    fn of(self, colour: Rgba, shade: f64) -> [u8; 4] {
        let mut colour = EntityColor {
            r: red_of(colour),
            g: green_of(colour),
            b: blue_of(colour),
            a: 255,
        };
        if self.white_to_black {
            colour = paper_colour(
                colour,
                &PlotSettings {
                    white_to_black: true,
                    colour_mode: PlotColourMode::Colour,
                    ..PlotSettings::default()
                },
            );
        }
        // +0.5 then truncate, as the 3D's shade() rounds.
        let channel = |value: u8| (f64::from(value) * shade + 0.5) as u8;
        let (r, g, b) = (channel(colour.r), channel(colour.g), channel(colour.b));
        if self.grey {
            let level = luminance(EntityColor { r, g, b, a: 255 });
            return [level, level, level, 255];
        }
        [r, g, b, 255]
    }
}

/// How the faces are painted: the medium's resolution and ink.
#[derive(Clone, Copy, Debug)]
pub struct MeshPaintOptions {
    pub paper: bool,
    /// On screen, device pixels per view pixel, so the image has a pixel per device pixel.
    pub device_ratio: f64,
    /// On paper, the share of the device's resolution the image is made at (the plan painter's
    /// imagery share: the imagery dpi cap), and at most this many pixels.
    pub imagery_share: f64,
    pub image_pixel_cap: usize,
    pub ink: MeshInk,
}

impl Default for MeshPaintOptions {
    fn default() -> Self {
        Self {
            paper: false,
            device_ratio: 1.0,
            imagery_share: 1.0,
            image_pixel_cap: 16_000_000,
            ink: MeshInk::SCREEN,
        }
    }
}

/// What one paint of the faces did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshFacesStats {
    /// Meshes with a face in the image.
    pub meshes_drawn: usize,
    /// Faces whose rows met the image.
    pub triangles_in_view: usize,
    /// The image's pixels, and how many of them a face or a rim lit.
    pub image_pixels: usize,
    pub pixels_covered: usize,
    /// Whether the image was made this paint (false: the kept one was laid down).
    pub image_made: bool,
}

// ---- what is kept between paints ------------------------------------------------------------

/// A mesh as the plan draws it, worked out once: its plan box, a shade a face and its rim. Keyed
/// on the mesh's address and size - the window holds each mesh shared and never edits one in
/// place (an imported mesh is replaced, not changed), so a mesh at the same address with the same
/// counts is the same mesh.
#[derive(Debug)]
struct MeshDisplay {
    address: usize,
    vertex_count: usize,
    face_count: usize,
    plan: Box2,
    /// MESH_LEVEL_SHADE and the hillshade, 0 to 255 of the face's colour.
    shades: Vec<u8>,
    rim: Vec<MeshEdge>,
}

impl MeshDisplay {
    fn of(mesh: &TriangleMesh, light: &SceneOptions<'_>) -> Self {
        let mut plan = Box2::default();
        for vertex in &mesh.vertices {
            if vertex.x.is_finite() && vertex.y.is_finite() {
                plan.expand(Point2::new(vertex.x, vertex.y));
            }
        }
        let level = hillshade(light, Vec3::new(0.0, 0.0, 1.0));
        let shade_of = |normal: Vec3| -> u8 {
            let share = if level > 0.0 {
                MESH_LEVEL_SHADE * hillshade(light, normal) / level
            } else {
                MESH_LEVEL_SHADE
            };
            (share.clamp(MESH_SHADE_FLOOR, 1.0) * 255.0 + 0.5) as u8
        };
        let level_shade = shade_of(Vec3::new(0.0, 0.0, 1.0));
        let shades = mesh
            .faces
            .iter()
            .map(|face| {
                let corner = |k: usize| mesh.vertices.get(face[k] as usize);
                let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
                    return level_shade;
                };
                let u = Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z);
                let v = Vec3::new(c.x - a.x, c.y - a.y, c.z - a.z);
                let normal = u.cross(v);
                let length = normal.length();
                if !(length > 0.0) || !length.is_finite() {
                    return level_shade;
                }
                // Seen from above whichever way the face is wound: a plan sees the upper side.
                let up = if normal.z < 0.0 { -length } else { length };
                shade_of(Vec3::new(normal.x / up, normal.y / up, normal.z / up))
            })
            .collect();
        Self {
            address: std::ptr::from_ref(mesh) as usize,
            vertex_count: mesh.vertices.len(),
            face_count: mesh.faces.len(),
            plan,
            shades,
            rim: mesh.boundary_edges(),
        }
    }

    fn is_of(&self, mesh: &TriangleMesh) -> bool {
        self.address == std::ptr::from_ref(mesh) as usize
            && self.vertex_count == mesh.vertices.len()
            && self.face_count == mesh.faces.len()
    }
}

/// The image last made, and what it was made from.
#[derive(Debug)]
struct KeptImage {
    key: ImageKey,
    grid: SurfacePlanImageGrid,
    image: Image,
    stats: MeshFacesStats,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ImageKey {
    /// The grid's box, as its bits.
    corners: [u64; 4],
    width: i32,
    height: i32,
    meshes: u64,
    ink: MeshInk,
}

/// What the faces keep between paints. One per view or plotting thread, inside the plan painter's
/// cache; never shared between threads.
#[derive(Debug, Default)]
pub struct MeshPlanCache {
    displays: Vec<MeshDisplay>,
    kept: Option<KeptImage>,
    images_made: usize,
    displays_made: usize,
}

impl MeshPlanCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.displays.clear();
        self.kept = None;
    }

    /// Images made since the cache was made: a paint that reuses the kept one does not count.
    pub fn images_made(&self) -> usize {
        self.images_made
    }

    /// Meshes worked out (box, shades, rim) since the cache was made.
    pub fn displays_made(&self) -> usize {
        self.displays_made
    }

    /// Makes `displays` one for each of `meshes`, in their order: the kept ones reused, the
    /// others made across the pool (each mesh's alone, so in any order), and those of meshes no
    /// longer drawn let go.
    fn prepare(&mut self, meshes: &[&TriangleMesh], pool: &TaskPool) {
        let mut kept: HashMap<usize, MeshDisplay> = self
            .displays
            .drain(..)
            .map(|display| (display.address, display))
            .collect();
        let mut found: Vec<Option<MeshDisplay>> = meshes
            .iter()
            .map(|mesh| {
                kept.remove(&(std::ptr::from_ref(*mesh) as usize))
                    .filter(|display| display.is_of(mesh))
            })
            .collect();
        let missing: Vec<usize> = (0..meshes.len()).filter(|&i| found[i].is_none()).collect();
        if !missing.is_empty() {
            let light = SceneOptions::default();
            let made: Vec<Mutex<Option<MeshDisplay>>> =
                missing.iter().map(|_| Mutex::new(None)).collect();
            pool.parallel_for(0, missing.len(), |k| {
                let display = MeshDisplay::of(meshes[missing[k]], &light);
                *made[k].lock().unwrap_or_else(PoisonError::into_inner) = Some(display);
            });
            for (k, slot) in made.into_iter().enumerate() {
                found[missing[k]] = slot.into_inner().unwrap_or_else(PoisonError::into_inner);
            }
            self.displays_made += missing.len();
        }
        self.displays = found
            .into_iter()
            .zip(meshes)
            .map(|(display, mesh)| {
                display.unwrap_or_else(|| MeshDisplay::of(mesh, &SceneOptions::default()))
            })
            .collect();
    }
}

// ---- the image ------------------------------------------------------------------------------

/// One mesh as the image reads it.
struct Placed<'a> {
    mesh: &'a TriangleMesh,
    display: &'a MeshDisplay,
    flat: Rgba,
    face_colours: &'a [Rgba],
    /// Its vertices in image pixels (x right, y down) and its heights.
    projected: Vec<[f64; 3]>,
}

impl Placed<'_> {
    fn colour(&self, face: usize) -> Rgba {
        self.face_colours.get(face).copied().unwrap_or(self.flat)
    }
}

/// A face that reaches the image: which, and its first and last rows there.
#[derive(Clone, Copy)]
struct Face {
    mesh: u32,
    face: u32,
    first: i32,
    last: i32,
}

/// A mesh the image meets: its vertices in image pixels with their heights, and its faces there.
type InView = (Vec<[f64; 3]>, Vec<Face>);

/// A band's rows of the three buffers.
struct Band<'p> {
    pixels: &'p mut [[u8; 4]],
    depth: &'p mut [f32],
    owner: &'p mut [u32],
}

fn clamp_index(value: f64, count: i32) -> i32 {
    if value.is_finite() {
        value.clamp(0.0, f64::from((count - 1).max(0))) as i32
    } else {
        0
    }
}

/// The meshes' faces in `grid`, as premultiplied RGBA pixels, row 0 at the grid box's top - the
/// image the plan painter lays down, made afresh (the cache's displays are used and kept, its
/// image is not). Public for the tests and the benchmark, which compare it across thread counts
/// and time it.
pub fn mesh_faces_image(
    meshes: &[&SceneMesh<'_>],
    grid: &SurfacePlanImageGrid,
    ink: MeshInk,
    cache: &mut MeshPlanCache,
    pool: &TaskPool,
) -> (Vec<[u8; 4]>, MeshFacesStats) {
    let width = grid.width;
    let height = grid.height;
    let mut stats = MeshFacesStats {
        image_made: true,
        ..MeshFacesStats::default()
    };
    if width <= 0 || height <= 0 || grid.r#box.empty() {
        return (Vec::new(), stats);
    }
    let count = width as usize * height as usize;
    stats.image_pixels = count;
    let size = grid.r#box.width() / f64::from(width);
    let x0 = grid.r#box.min.x;
    let y0 = grid.r#box.max.y;

    let items: Vec<&SceneMesh<'_>> = meshes
        .iter()
        .copied()
        .filter(|item| item.mesh.is_some())
        .collect();
    let shown: Vec<&TriangleMesh> = items.iter().filter_map(|item| item.mesh).collect();
    cache.prepare(&shown, pool);

    // The meshes the image meets, their vertices in image pixels and their faces' rows - each
    // mesh's alone, across the pool.
    let candidates: Vec<usize> = (0..shown.len())
        .filter(|&i| {
            let plan = cache.displays[i].plan;
            !plan.empty() && plan.intersects(grid.r#box)
        })
        .collect();
    let slots: Vec<Mutex<Option<InView>>> = candidates.iter().map(|_| Mutex::new(None)).collect();
    pool.parallel_for(0, candidates.len(), |k| {
        let i = candidates[k];
        let mesh = shown[i];
        let projected: Vec<[f64; 3]> = mesh
            .vertices
            .iter()
            .map(|v| [(v.x - x0) / size, (y0 - v.y) / size, v.z])
            .collect();
        let mut faces = Vec::new();
        for (f, corners) in mesh.faces.iter().enumerate() {
            let (Some(a), Some(b), Some(c)) = (
                projected.get(corners[0] as usize),
                projected.get(corners[1] as usize),
                projected.get(corners[2] as usize),
            ) else {
                continue;
            };
            // A face with a corner that is not a number has no place to be drawn.
            if !a.iter().chain(b).chain(c).all(|value| value.is_finite()) {
                continue;
            }
            let top = a[1].min(b[1]).min(c[1]);
            let bottom = a[1].max(b[1]).max(c[1]);
            let left = a[0].min(b[0]).min(c[0]);
            let right = a[0].max(b[0]).max(c[0]);
            if !(bottom >= 0.0
                && top <= f64::from(height)
                && right >= 0.0
                && left <= f64::from(width))
            {
                continue; // off the image, or not finite
            }
            let (first, last) = rows_of(top, bottom, height);
            faces.push(Face {
                mesh: k as u32,
                face: f as u32,
                first,
                last,
            });
        }
        *slots[k].lock().unwrap_or_else(PoisonError::into_inner) = Some((projected, faces));
    });
    let mut placed: Vec<Placed<'_>> = Vec::with_capacity(candidates.len());
    let mut faces: Vec<Face> = Vec::new();
    for (k, slot) in slots.into_iter().enumerate() {
        let (projected, mesh_faces) = slot
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
            .unwrap_or_default();
        let i = candidates[k];
        if !mesh_faces.is_empty() {
            stats.meshes_drawn += 1;
        }
        faces.extend(mesh_faces);
        placed.push(Placed {
            mesh: shown[i],
            display: &cache.displays[i],
            flat: items[i].flat_color,
            face_colours: &items[i].face_colors,
            projected,
        });
    }
    stats.triangles_in_view = faces.len();

    // Bin the faces into bands of rows, in the meshes' order.
    let band_count = ((height + BAND_ROWS - 1) / BAND_ROWS) as usize;
    let mut start = vec![0usize; band_count + 1];
    for face in &faces {
        for band in face.first / BAND_ROWS..=face.last / BAND_ROWS {
            start[band as usize + 1] += 1;
        }
    }
    for band in 0..band_count {
        start[band + 1] += start[band];
    }
    let mut binned = vec![0u32; start[band_count]];
    let mut fill = start[..band_count].to_vec();
    for (i, face) in faces.iter().enumerate() {
        for band in face.first / BAND_ROWS..=face.last / BAND_ROWS {
            let slot = &mut fill[band as usize];
            binned[*slot] = i as u32;
            *slot += 1;
        }
    }

    let mut pixels = vec![[0u8; 4]; count];
    let mut depth = vec![f32::NEG_INFINITY; count];
    let mut owner = vec![0u32; count];
    {
        let rows = BAND_ROWS as usize * width as usize;
        let bands: Vec<Mutex<Band<'_>>> = pixels
            .chunks_mut(rows)
            .zip(depth.chunks_mut(rows))
            .zip(owner.chunks_mut(rows))
            .map(|((pixels, depth), owner)| {
                Mutex::new(Band {
                    pixels,
                    depth,
                    owner,
                })
            })
            .collect();
        pool.parallel_ranges(0, band_count, 1, |lo, hi| {
            for band in lo..hi {
                let mut guard = bands[band].lock().unwrap_or_else(PoisonError::into_inner);
                let row_lo = band as i32 * BAND_ROWS;
                let row_hi = (row_lo + BAND_ROWS - 1).min(height - 1);
                for &i in &binned[start[band]..start[band + 1]] {
                    let face = faces[i as usize];
                    rasterise(
                        &mut guard,
                        row_lo,
                        face.first.max(row_lo),
                        face.last.min(row_hi),
                        width,
                        &placed[face.mesh as usize],
                        face,
                        ink,
                    );
                }
            }
        });
    }

    // The rims, over the faces, one mesh after another: a rim pixel is drawn where its own mesh is
    // the one seen, or nothing is, so a pavement's edge does not show through the kerb on it.
    for (k, item) in placed.iter().enumerate() {
        let across = (item.display.plan.width().max(item.display.plan.height())) / size;
        if item.display.rim.is_empty() || !(across >= MESH_RIM_MINIMUM_PIXELS) {
            continue;
        }
        let me = k as u32 + 1;
        for edge in &item.display.rim {
            let (Some(a), Some(b)) = (
                item.projected.get(edge.a as usize),
                item.projected.get(edge.b as usize),
            ) else {
                continue;
            };
            let shade = f64::from(item.display.shades[edge.face as usize]) / 255.0;
            let colour = ink.of(item.colour(edge.face as usize), shade * MESH_RIM_SHADE);
            trace_line([a[0], a[1]], [b[0], b[1]], width, height, |at| {
                if owner[at] == 0 || owner[at] == me {
                    pixels[at] = colour;
                    owner[at] = me;
                }
            });
        }
    }
    stats.pixels_covered = owner.iter().filter(|&&o| o != 0).count();
    (pixels, stats)
}

/// The rows whose centres a face from `top` to `bottom` (image y) crosses; a face between two row
/// centres gets the row its middle is in, so it is not lost.
fn rows_of(top: f64, bottom: f64, height: i32) -> (i32, i32) {
    let first = (top - 0.5).ceil();
    let last = (bottom - 0.5).floor();
    if first <= last {
        (clamp_index(first, height), clamp_index(last, height))
    } else {
        let row = clamp_index((0.5 * (top + bottom)).floor(), height);
        (row, row)
    }
}

/// Every pixel the segment from `a` to `b` (image pixels) passes through, a step at a time along
/// its longer axis, clipped to the image first so a rim that runs far off a view zoomed in costs
/// what its visible part does.
fn trace_line(a: [f64; 2], b: [f64; 2], width: i32, height: i32, mut put: impl FnMut(usize)) {
    // Liang-Barsky against the image, a pixel of margin.
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    for (p, q) in [
        (-dx, a[0] + 1.0),
        (dx, f64::from(width) + 1.0 - a[0]),
        (-dy, a[1] + 1.0),
        (dy, f64::from(height) + 1.0 - a[1]),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    if !(t0 <= t1) {
        return;
    }
    let (sx, sy) = (a[0] + t0 * dx, a[1] + t0 * dy);
    let (ex, ey) = (a[0] + t1 * dx, a[1] + t1 * dy);
    let steps = ((ex - sx).abs().max((ey - sy).abs()).ceil() as usize).max(1);
    for step in 0..=steps {
        let t = step as f64 / steps as f64;
        let x = (sx + t * (ex - sx)).floor();
        let y = (sy + t * (ey - sy)).floor();
        if (0.0..f64::from(width)).contains(&x) && (0.0..f64::from(height)).contains(&y) {
            put(y as usize * width as usize + x as usize);
        }
    }
}

/// Face `face` of `item` in rows `first` to `last` of a band whose first row is `row_lo`: each
/// pixel whose centre it covers, at its height in the plane of its corners, written where it is no
/// lower than what the pixel holds (later faces win a tie, so the order decides it, never the
/// threads). In a row it crosses without covering a centre - a sliver - the pixel nearest its
/// middle is written instead.
#[allow(
    clippy::too_many_arguments,
    reason = "The band, its rows, the image's width, the face and the ink: one call a face a band."
)]
fn rasterise(
    band: &mut Band<'_>,
    row_lo: i32,
    first: i32,
    last: i32,
    width: i32,
    item: &Placed<'_>,
    face: Face,
    ink: MeshInk,
) {
    if first > last {
        return;
    }
    let f = face.face as usize;
    let corners = item.mesh.faces[f];
    let p = [
        item.projected[corners[0] as usize],
        item.projected[corners[1] as usize],
        item.projected[corners[2] as usize],
    ];
    let shade = f64::from(item.display.shades[f]) / 255.0;
    let colour = ink.of(item.colour(f), shade);
    let me = face.mesh + 1;
    let z_low = p[0][2].min(p[1][2]).min(p[2][2]);
    let z_high = p[0][2].max(p[1][2]).max(p[2][2]);
    // The plane z = z0 + gx (x - x0) + gy (y - y0) through the corners; a face seen edge on (a
    // wall) has none, and is drawn at its top.
    let area2 =
        (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
    let (gx, gy) = if area2.abs() > 1.0e-9 && area2.is_finite() {
        let dz1 = p[1][2] - p[0][2];
        let dz2 = p[2][2] - p[0][2];
        (
            (dz1 * (p[2][1] - p[0][1]) - dz2 * (p[1][1] - p[0][1])) / area2,
            (dz2 * (p[1][0] - p[0][0]) - dz1 * (p[2][0] - p[0][0])) / area2,
        )
    } else {
        (f64::NAN, f64::NAN)
    };
    let height_at = |x: f64, y: f64| -> f32 {
        if gx.is_finite() && gy.is_finite() {
            (p[0][2] + gx * (x - p[0][0]) + gy * (y - p[0][1])).clamp(z_low, z_high) as f32
        } else {
            z_high as f32
        }
    };
    let top = p[0][1].min(p[1][1]).min(p[2][1]);
    let bottom = p[0][1].max(p[1][1]).max(p[2][1]);
    // A face between two row centres was given the row its middle is in: it is spanned there by
    // its whole width.
    let between_rows = (top - 0.5).ceil() > (bottom - 0.5).floor();
    let w = width as usize;
    let put = |band: &mut Band<'_>, row: i32, column: i32, x: f64, y: f64| {
        let at = (row - row_lo) as usize * w + column as usize;
        let z = height_at(x, y);
        if z >= band.depth[at] {
            band.depth[at] = z;
            band.pixels[at] = colour;
            band.owner[at] = me;
        }
    };
    for row in first..=last {
        let y = f64::from(row) + 0.5;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        if between_rows {
            for corner in &p {
                lo = lo.min(corner[0]);
                hi = hi.max(corner[0]);
            }
        } else {
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                if (a[1].min(b[1])..=a[1].max(b[1])).contains(&y) {
                    if a[1] == b[1] {
                        lo = lo.min(a[0]).min(b[0]);
                        hi = hi.max(a[0]).max(b[0]);
                    } else {
                        let x = a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                }
            }
        }
        if !(lo <= hi) || hi < 0.0 || lo > f64::from(width) {
            continue;
        }
        let column_lo = (lo - 0.5).ceil();
        let column_hi = (hi - 0.5).floor();
        if column_lo > column_hi {
            // A sliver in this row: the pixel nearest its middle.
            let middle = 0.5 * (lo + hi);
            let column = middle.floor();
            if (0.0..f64::from(width)).contains(&column) {
                put(band, row, column as i32, middle, y);
            }
            continue;
        }
        let from = clamp_index(column_lo, width);
        let to = clamp_index(column_hi, width);
        if column_hi < 0.0 || column_lo > f64::from(width - 1) {
            continue;
        }
        for column in from..=to {
            put(band, row, column, f64::from(column) + 0.5, y);
        }
    }
}

// ---- painting -------------------------------------------------------------------------------

/// FNV-1a over what the image depends on of the meshes: each one's address and size and its
/// colours - the flat one, and how many per-face ones (not where they are: the window lends a
/// fresh copy of its list every frame, and a mesh's face colours are its file's, never restyled).
fn fingerprint(meshes: &[&SceneMesh<'_>]) -> u64 {
    let mut value: u64 = 0xcbf2_9ce4_8422_2325;
    let mut add = |word: u64| {
        for byte in word.to_le_bytes() {
            value ^= u64::from(byte);
            value = value.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    add(meshes.len() as u64);
    for item in meshes {
        let Some(mesh) = item.mesh else {
            add(0);
            continue;
        };
        add(std::ptr::from_ref(mesh) as usize as u64);
        add(mesh.vertices.len() as u64);
        add(mesh.faces.len() as u64);
        add(u64::from(item.flat_color));
        add(item.face_colors.len() as u64);
    }
    value
}

fn image_of(width: i32, height: i32, pixels: Vec<[u8; 4]>) -> Option<Image> {
    Image::from_bytes(
        u32::try_from(width).ok()?,
        u32::try_from(height).ok()?,
        ImageFormat::Rgba8Premultiplied,
        pixels.into_flattened(),
    )
    .ok()
}

/// The faces of `meshes` - those the plan draws, in their order - seen through `view` (model to
/// the viewport's pixels, y down) on a canvas that already carries the frame's origin and turn.
/// `visible` is the model box the frame shows (the plan painter's visible_box). The image is
/// kept in `cache` and laid down again while nothing it was made from has changed. The canvas's
/// state is the same afterwards.
pub fn paint_meshes_in_plan(
    canvas: &mut dyn Canvas,
    meshes: &[&SceneMesh<'_>],
    view: &ViewTransform,
    visible: &Box2,
    options: &MeshPaintOptions,
    cache: &mut MeshPlanCache,
) -> MeshFacesStats {
    let shown: Vec<&TriangleMesh> = meshes.iter().filter_map(|item| item.mesh).collect();
    if shown.is_empty() || !(view.scale > 0.0) || visible.empty() {
        cache.clear();
        return MeshFacesStats::default();
    }
    // The plan boxes are the displays', worked out once a mesh.
    cache.prepare(&shown, TaskPool::shared());
    let mut bounds = Box2::default();
    for display in &cache.displays {
        bounds.expand_box(display.plan);
    }
    // An image pixel: a device pixel on screen; on paper the device's pixel at the imagery share
    // (the dpi cap) - a surface's faces' rule.
    let image_pixel = if options.paper {
        1.0 / (view.scale * options.imagery_share.clamp(1.0e-6, 1.0))
    } else {
        1.0 / (view.scale * options.device_ratio.max(1.0e-6))
    };
    let grid = plan_image_grid(visible, &bounds, image_pixel, options.image_pixel_cap);
    if grid.width <= 0 || grid.height <= 0 {
        return MeshFacesStats::default();
    }
    let key = ImageKey {
        corners: [
            grid.r#box.min.x.to_bits(),
            grid.r#box.min.y.to_bits(),
            grid.r#box.max.x.to_bits(),
            grid.r#box.max.y.to_bits(),
        ],
        width: grid.width,
        height: grid.height,
        meshes: fingerprint(meshes),
        ink: options.ink,
    };
    let reused = cache.kept.as_ref().is_some_and(|kept| kept.key == key);
    if !reused {
        cache.kept = None;
        let (pixels, stats) =
            mesh_faces_image(meshes, &grid, options.ink, cache, TaskPool::shared());
        cache.images_made += 1;
        if stats.pixels_covered > 0
            && let Some(image) = image_of(grid.width, grid.height, pixels)
        {
            cache.kept = Some(KeptImage {
                key,
                grid,
                image,
                stats,
            });
        } else {
            return stats;
        }
    }
    let Some(kept) = cache.kept.as_ref() else {
        return MeshFacesStats::default();
    };
    let size = kept.grid.r#box.width() / f64::from(kept.grid.width);
    let corner = view.world_to_screen(Point2::new(kept.grid.r#box.min.x, kept.grid.r#box.max.y));
    let step = size * view.scale;
    canvas.save();
    canvas.set_world_transform(
        Transform::new(step, 0.0, 0.0, step, corner.x, corner.y),
        true,
    );
    // Smoothed only on paper, where the image is coarser than the device; on screen each image
    // pixel is a device pixel.
    canvas.set_render_hint(RenderHint::SmoothPixmapTransform, options.paper);
    if options.paper {
        canvas.set_opacity(MESH_FACE_PAPER_OPACITY);
    }
    canvas.draw_image_at(PointF::new(0.0, 0.0), &kept.image);
    canvas.restore();
    MeshFacesStats {
        image_made: !reused,
        ..kept.stats
    }
}
