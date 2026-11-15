//! The soft renderer: the picture of a headless frame, drawn without a GPU (the plan's section 4.5),
//! which is what `--screenshot` writes and what a test reads pixels from.
//!
//! WHAT IT DRAWS. egui's tessellated output - `ClippedPrimitive`s of vertex-coloured, textured
//! triangles, each with a clip rectangle - into an `Image` of the screen's device pixels, with the
//! textures the frames delivered (`TexturesDelta`: the font atlas, and the plan view's and the
//! icons' images). It is egui-wgpu's work done on the CPU: the C++'s offscreen runs need no device,
//! and neither may a headless run of this program or a test that writes a PNG.
//!
//! THE RULES, egui-wgpu's own (egui.wgsl and renderer.rs, 0.36.2), so that what is drawn here is
//! what the desktop draws, give or take the GPU's rounding:
//!
//! ```text
//! positions       points times pixels per point; a pixel is covered when its centre is inside
//!                 a triangle; a centre on an edge belongs to one of the two triangles sharing
//!                 it (below), so a mesh's seams are never blended twice
//! colour          the vertex colours interpolated across the triangle, times the texture's
//!                 texel, both sRGB-encoded and premultiplied, in 0..1 - egui's
//!                 fs_main_gamma_framebuffer, with no dithering
//! blending        premultiplied: dst = src + dst (1 - src.a) for the colour, and
//!                 dst.a = src.a (1 - dst.a) + dst.a for the alpha (egui-wgpu's BlendState)
//! the clip        the clip rectangle times pixels per point, each corner rounded to a whole
//!                 pixel and kept inside the target (egui-wgpu's ScissorRect)
//! texels          bilinear at texel centres, as egui-wgpu's predictable filtering takes them,
//!                 or nearest, by the texture's options; magnification or minification chosen
//!                 by whether a triangle maps more than one texel to a pixel; clamped,
//!                 repeated or mirrored by the texture's wrap mode
//! paint callbacks drawn by a GPU only: a headless run has none (the 3D view takes its
//!                 software path headless, RendererChoice's rule), and one met is left undrawn
//! ```
//!
//! The edge rule. A pixel centre exactly on an edge is inside when that edge, walked with the
//! triangle turned one way, goes down the screen, or across it to the right. Two triangles sharing
//! an edge walk it in opposite directions, so exactly one of them takes such a pixel - the
//! top-left rule's purpose, by an equivalent tie-break.
//!
//! IT IS NOT egui-wgpu's output bit for bit, and need not be: no test compares a screenshot's pixels
//! (the gate keeps both programs' pictures for a person to look at), and the PNG's size floors are
//! the check that something was painted. A test that reads pixels reads what this draws, against
//! a baseline drawn the same way.
//!
//! THE CROP. `--screenshot` writes the target the steps left - the window, or a dialog, dock,
//! panel or popup - which the registry records in points, its frame included (drive::registry).
//! Its pixels are that rectangle times pixels per point, the corners rounded OUTWARD to whole
//! pixels (the left and top down, the right and bottom up), so no edge of the target is cut, and
//! then kept inside the screen: `crop_rect`.

use std::collections::HashMap;

use egui::epaint::{ImageDelta, Primitive, Vertex};
use egui::{
    ClippedPrimitive, Color32, ImageData, Pos2, Rect, TextureFilter, TextureId, TextureOptions,
    TextureWrapMode, TexturesDelta,
};

use crate::canvas::{Image, ImageFormat, IntRect};

/// A colour in 0..1 a channel, premultiplied, sRGB-encoded: what egui's shader computes in.
type Unit = [f32; 4];

fn unit(color: Color32) -> Unit {
    let [r, g, b, a] = color.to_array();
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        f32::from(a) / 255.0,
    ]
}

/// A channel back to eight bits, rounded to nearest, as a GPU stores a unorm channel.
fn byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// One texture as the frames delivered it.
#[derive(Clone, Debug)]
struct Texture {
    width: usize,
    height: usize,
    /// Row after row from the top, premultiplied, as egui keeps them.
    pixels: Vec<Color32>,
    options: TextureOptions,
}

impl Texture {
    fn texel(&self, x: i64, y: i64) -> Unit {
        let wrap = self.options.wrap_mode;
        let x = wrapped(x, self.width, wrap);
        let y = wrapped(y, self.height, wrap);
        unit(self.pixels[y * self.width + x])
    }

    /// The texel at `uv` (0..1 across the texture) by `filter`.
    #[allow(
        clippy::needless_range_loop,
        reason = "a channel's index picks the same channel of four taps"
    )]
    fn sample(&self, uv: Pos2, filter: TextureFilter) -> Unit {
        if self.width == 0 || self.height == 0 {
            return [1.0; 4];
        }
        let x = uv.x * self.width as f32;
        let y = uv.y * self.height as f32;
        match filter {
            TextureFilter::Nearest => self.texel(x.floor() as i64, y.floor() as i64),
            TextureFilter::Linear => {
                // At texel centres, four taps mixed (egui.wgsl's manual bilinear filtering).
                let x = x - 0.5;
                let y = y - 0.5;
                let (left, top) = (x.floor(), y.floor());
                let (fx, fy) = (x - left, y - top);
                let (left, top) = (left as i64, top as i64);
                let tl = self.texel(left, top);
                let tr = self.texel(left + 1, top);
                let bl = self.texel(left, top + 1);
                let br = self.texel(left + 1, top + 1);
                let mut out = [0.0; 4];
                for channel in 0..4 {
                    let upper = tl[channel] + (tr[channel] - tl[channel]) * fx;
                    let lower = bl[channel] + (br[channel] - bl[channel]) * fx;
                    out[channel] = upper + (lower - upper) * fy;
                }
                out
            }
        }
    }
}

/// A texel index brought into 0..size by the wrap mode.
fn wrapped(at: i64, size: usize, wrap: TextureWrapMode) -> usize {
    let size = size as i64;
    let index = match wrap {
        TextureWrapMode::ClampToEdge => at.clamp(0, size - 1),
        TextureWrapMode::Repeat => at.rem_euclid(size),
        TextureWrapMode::MirroredRepeat => {
            let period = at.rem_euclid(2 * size);
            if period < size {
                period
            } else {
                2 * size - 1 - period
            }
        }
    };
    index as usize
}

/// The pixels a primitive may touch: egui-wgpu's ScissorRect, as whole pixels `x0..x1` by
/// `y0..y1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scissor {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

fn scissor(clip: Rect, pixels_per_point: f32, width: usize, height: usize) -> Scissor {
    // Rounded to whole pixels, then clamped: a negative corner is the target's edge (egui-wgpu
    // casts the rounded float to u32, which saturates a negative to 0).
    let corner = |value: f32, most: usize| -> usize {
        let rounded = (value * pixels_per_point).round();
        if rounded <= 0.0 {
            0
        } else {
            (rounded as usize).min(most)
        }
    };
    let x0 = corner(clip.min.x, width);
    let y0 = corner(clip.min.y, height);
    let x1 = corner(clip.max.x, width).max(x0);
    let y1 = corner(clip.max.y, height).max(y0);
    Scissor { x0, y0, x1, y1 }
}

/// The pixels of `rect` (points) at `pixels_per_point`, its corners rounded outward and kept inside
/// a screen of `width` by `height` pixels: what a screenshot of a target writes (module comment).
pub fn crop_rect(rect: Rect, pixels_per_point: f32, width: u32, height: u32) -> IntRect {
    let clamp = |value: f32, most: u32| -> i32 {
        if value <= 0.0 {
            0
        } else {
            (value as i64).min(i64::from(most)) as i32
        }
    };
    let left = clamp((rect.min.x * pixels_per_point).floor(), width);
    let top = clamp((rect.min.y * pixels_per_point).floor(), height);
    let right = clamp((rect.max.x * pixels_per_point).ceil(), width).max(left);
    let bottom = clamp((rect.max.y * pixels_per_point).ceil(), height).max(top);
    IntRect::new(left, top, right - left, bottom - top)
}

/// Twice a triangle's signed area from `a` to `b` to `c`: positive when `c` is on the side of `a`
/// to `b` that a turn from x towards y makes.
fn edge(a: Pos2, b: Pos2, c: Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Whether a pixel centre exactly on the edge `a` to `b` is the triangle's (the edge rule above).
fn owns(a: Pos2, b: Pos2) -> bool {
    let along = b - a;
    along.y > 0.0 || (along.y == 0.0 && along.x > 0.0)
}

/// The textures a run's frames delivered, and the rasteriser that draws a frame with them.
#[derive(Clone, Debug, Default)]
pub struct SoftRenderer {
    textures: HashMap<TextureId, Texture>,
}

impl SoftRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many textures it holds.
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Whether it holds texture `id`.
    pub fn has_texture(&self, id: TextureId) -> bool {
        self.textures.contains_key(&id)
    }

    /// A frame's new and changed textures, applied before it is drawn.
    pub fn set_textures(&mut self, delta: &TexturesDelta) {
        for (id, changes) in &delta.set {
            for change in changes.iter() {
                self.set_texture(*id, change);
            }
        }
    }

    /// A frame's freed textures, dropped after it is drawn.
    pub fn free_textures(&mut self, delta: &TexturesDelta) {
        for id in &delta.free {
            self.textures.remove(id);
        }
    }

    /// One texture dropped.
    pub fn free_texture(&mut self, id: TextureId) {
        self.textures.remove(&id);
    }

    /// One texture set whole, or a patch of it written at its place.
    pub fn set_texture(&mut self, id: TextureId, delta: &ImageDelta) {
        let ImageData::Color(image) = &delta.image;
        let [width, height] = image.size;
        match delta.pos {
            None => {
                self.textures.insert(
                    id,
                    Texture {
                        width,
                        height,
                        pixels: image.pixels.clone(),
                        options: delta.options,
                    },
                );
            }
            Some([x, y]) => {
                // A patch of a texture never set whole is egui-wgpu's error too; nothing to
                // patch.
                let Some(texture) = self.textures.get_mut(&id) else {
                    return;
                };
                texture.options = delta.options;
                for row in 0..height {
                    let into_y = y + row;
                    if into_y >= texture.height {
                        break;
                    }
                    for column in 0..width {
                        let into_x = x + column;
                        if into_x >= texture.width {
                            break;
                        }
                        texture.pixels[into_y * texture.width + into_x] =
                            image.pixels[row * width + column];
                    }
                }
            }
        }
    }

    /// The image `primitives` draw on a target of `width` by `height` device pixels at
    /// `pixels_per_point`, cleared to `clear` first: RGBA premultiplied, at that ratio.
    pub fn render(
        &self,
        primitives: &[ClippedPrimitive],
        width: u32,
        height: u32,
        pixels_per_point: f32,
        clear: Color32,
    ) -> Image {
        let (columns, rows) = (width as usize, height as usize);
        let mut target = vec![unit(clear); columns * rows];
        for primitive in primitives {
            let Primitive::Mesh(mesh) = &primitive.primitive else {
                continue;
            };
            let Some(texture) = self.textures.get(&mesh.texture_id) else {
                continue;
            };
            let clip = scissor(primitive.clip_rect, pixels_per_point, columns, rows);
            if clip.x0 >= clip.x1 || clip.y0 >= clip.y1 {
                continue;
            }
            for triangle in mesh.indices.as_chunks::<3>().0 {
                let corners = [
                    mesh.vertices.get(triangle[0] as usize),
                    mesh.vertices.get(triangle[1] as usize),
                    mesh.vertices.get(triangle[2] as usize),
                ];
                if let [Some(a), Some(b), Some(c)] = corners {
                    fill_triangle(
                        &mut target,
                        columns,
                        clip,
                        pixels_per_point,
                        [a, b, c],
                        texture,
                    );
                }
            }
        }
        let mut data = Vec::with_capacity(columns * rows * 4);
        for pixel in &target {
            data.extend_from_slice(&[
                byte(pixel[0]),
                byte(pixel[1]),
                byte(pixel[2]),
                byte(pixel[3]),
            ]);
        }
        let mut image = Image::from_bytes(width, height, ImageFormat::Rgba8Premultiplied, data)
            .expect("the target holds four bytes for each of its pixels");
        image.set_device_pixel_ratio(f64::from(pixels_per_point));
        image
    }
}

/// One triangle drawn into `target` (`columns` pixels a row) inside `clip`.
#[allow(
    clippy::needless_range_loop,
    reason = "a channel's index picks the same channel of the corners, the texel and the target"
)]
fn fill_triangle(
    target: &mut [Unit],
    columns: usize,
    clip: Scissor,
    pixels_per_point: f32,
    vertices: [&Vertex; 3],
    texture: &Texture,
) {
    let mut corners = vertices;
    let mut p = corners.map(|vertex| {
        Pos2::new(
            vertex.pos.x * pixels_per_point,
            vertex.pos.y * pixels_per_point,
        )
    });
    let mut area = edge(p[0], p[1], p[2]);
    if area == 0.0 || !area.is_finite() {
        return;
    }
    if area < 0.0 {
        // One turn for every triangle, so the edge rule means the same for all of them.
        corners.swap(1, 2);
        p.swap(1, 2);
        area = -area;
    }
    let lowest_x = p[0].x.min(p[1].x).min(p[2].x).floor();
    let highest_x = p[0].x.max(p[1].x).max(p[2].x).ceil();
    let lowest_y = p[0].y.min(p[1].y).min(p[2].y).floor();
    let highest_y = p[0].y.max(p[1].y).max(p[2].y).ceil();
    let bound = |value: f32, low: usize, high: usize| -> usize {
        if value <= low as f32 {
            low
        } else {
            (value as usize).clamp(low, high)
        }
    };
    let x0 = bound(lowest_x, clip.x0, clip.x1);
    let x1 = bound(highest_x, clip.x0, clip.x1);
    let y0 = bound(lowest_y, clip.y0, clip.y1);
    let y1 = bound(highest_y, clip.y0, clip.y1);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    // Each weight is linear across the screen; its steps say how far a texel moves for a pixel.
    let weight_steps = [
        (-(p[2].y - p[1].y), p[2].x - p[1].x),
        (-(p[0].y - p[2].y), p[0].x - p[2].x),
        (-(p[1].y - p[0].y), p[1].x - p[0].x),
    ];
    let uv = corners.map(|vertex| vertex.uv);
    let mut du = (0.0_f32, 0.0_f32);
    let mut dv = (0.0_f32, 0.0_f32);
    for (corner, (step_x, step_y)) in uv.iter().zip(weight_steps) {
        du.0 += corner.x * step_x / area;
        du.1 += corner.x * step_y / area;
        dv.0 += corner.y * step_x / area;
        dv.1 += corner.y * step_y / area;
    }
    let footprint = (du.0.abs() * texture.width as f32)
        .max(du.1.abs() * texture.width as f32)
        .max(dv.0.abs() * texture.height as f32)
        .max(dv.1.abs() * texture.height as f32);
    let filter = if footprint > 1.0 {
        texture.options.minification
    } else {
        texture.options.magnification
    };
    let colours = corners.map(|vertex| unit(vertex.color));
    let owned = [owns(p[1], p[2]), owns(p[2], p[0]), owns(p[0], p[1])];

    for y in y0..y1 {
        for x in x0..x1 {
            let centre = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            let weights = [
                edge(p[1], p[2], centre),
                edge(p[2], p[0], centre),
                edge(p[0], p[1], centre),
            ];
            let inside = weights
                .iter()
                .zip(owned)
                .all(|(&weight, takes)| weight > 0.0 || (weight == 0.0 && takes));
            if !inside {
                continue;
            }
            let l = weights.map(|weight| weight / area);
            let at = Pos2::new(
                l[0] * uv[0].x + l[1] * uv[1].x + l[2] * uv[2].x,
                l[0] * uv[0].y + l[1] * uv[1].y + l[2] * uv[2].y,
            );
            let texel = texture.sample(at, filter);
            let mut source = [0.0_f32; 4];
            for channel in 0..4 {
                let colour = l[0] * colours[0][channel]
                    + l[1] * colours[1][channel]
                    + l[2] * colours[2][channel];
                source[channel] = colour * texel[channel];
            }
            let destination = &mut target[y * columns + x];
            let keep = 1.0 - source[3];
            for channel in 0..3 {
                destination[channel] = source[channel] + destination[channel] * keep;
            }
            destination[3] = source[3] * (1.0 - destination[3]) + destination[3];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{crop_rect, wrapped};
    use egui::{Rect, TextureWrapMode, pos2};

    use crate::canvas::IntRect;

    // Worked by hand at 1.25: 10.4 is 13.0 and 10.6 is 13.25, both down to 13; 20.2 is 25.25 and
    // 20.5 is 25.625, both up to 26.
    #[test]
    fn a_crop_takes_every_pixel_its_target_touches() {
        let rect = Rect::from_min_max(pos2(10.4, 10.6), pos2(20.2, 20.5));
        assert_eq!(
            crop_rect(rect, 1.25, 100, 100),
            IntRect::new(13, 13, 13, 13)
        );
        let beyond = Rect::from_min_max(pos2(-5.0, -5.0), pos2(500.0, 500.0));
        assert_eq!(crop_rect(beyond, 1.0, 40, 30), IntRect::new(0, 0, 40, 30));
    }

    #[test]
    fn a_texel_beyond_the_edge_is_clamped_repeated_or_mirrored() {
        assert_eq!(wrapped(-1, 4, TextureWrapMode::ClampToEdge), 0);
        assert_eq!(wrapped(5, 4, TextureWrapMode::ClampToEdge), 3);
        assert_eq!(wrapped(5, 4, TextureWrapMode::Repeat), 1);
        assert_eq!(wrapped(-1, 4, TextureWrapMode::Repeat), 3);
        assert_eq!(wrapped(4, 4, TextureWrapMode::MirroredRepeat), 3);
        assert_eq!(wrapped(-1, 4, TextureWrapMode::MirroredRepeat), 0);
    }
}
