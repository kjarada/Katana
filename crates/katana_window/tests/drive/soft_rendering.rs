//! The soft renderer's own cases (drive::soft_renderer): hand-made meshes whose every pixel is
//! worked by hand - coverage by pixel centres, a shared edge blended once, premultiplied blending,
//! texels nearest, the clip, the scale - and then egui's own frames through the headless runner:
//! text from the font atlas egui delivers, and a target cropped to its rectangle.

use std::sync::Arc;

use egui::epaint::{ImageDelta, Primitive, Vertex};
use egui::{
    ClippedPrimitive, Color32, ColorImage, ImageData, Mesh, Rect, TextureId, TextureOptions, pos2,
};
use katana_window::canvas::{Color, IntRect};
use katana_window::drive::headless::Headless;
use katana_window::drive::soft_renderer::{SoftRenderer, crop_rect};
use katana_window::drive::{TargetKind, TargetSpec, begin_target, end_target};

mod soft_renderer {
    use super::*;

    /// The texture every untextured shape samples: one white texel.
    const WHITE: TextureId = TextureId::Managed(0);

    fn texture(
        id: TextureId,
        width: usize,
        pixels: Vec<Color32>,
        options: TextureOptions,
    ) -> (TextureId, ImageDelta) {
        let height = pixels.len() / width;
        let image = ColorImage::new([width, height], pixels);
        (
            id,
            ImageDelta::full(ImageData::Color(Arc::new(image)), options),
        )
    }

    fn renderer() -> SoftRenderer {
        let mut renderer = SoftRenderer::new();
        let (id, delta) = texture(WHITE, 1, vec![Color32::WHITE], TextureOptions::NEAREST);
        renderer.set_texture(id, &delta);
        renderer
    }

    fn vertex(x: f32, y: f32, u: f32, v: f32, color: Color32) -> Vertex {
        Vertex {
            pos: pos2(x, y),
            uv: pos2(u, v),
            color,
        }
    }

    /// The quad `rect` in `color`, two triangles sharing the diagonal from its top left, each
    /// corner mapping the texture's corner.
    fn quad(rect: Rect, color: Color32, texture: TextureId) -> Mesh {
        let mut mesh = Mesh::with_texture(texture);
        mesh.vertices.extend([
            vertex(rect.min.x, rect.min.y, 0.0, 0.0, color),
            vertex(rect.max.x, rect.min.y, 1.0, 0.0, color),
            vertex(rect.max.x, rect.max.y, 1.0, 1.0, color),
            vertex(rect.min.x, rect.max.y, 0.0, 1.0, color),
        ]);
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        mesh
    }

    fn primitive(mesh: Mesh, clip: Rect) -> ClippedPrimitive {
        ClippedPrimitive {
            clip_rect: clip,
            primitive: Primitive::Mesh(mesh),
        }
    }

    fn everywhere() -> Rect {
        Rect::from_min_max(pos2(-1000.0, -1000.0), pos2(1000.0, 1000.0))
    }

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    }

    /// How many pixels of the image are `color`.
    fn count(image: &katana_window::canvas::Image, color: Color) -> usize {
        let mut found = 0;
        for y in 0..image.height() {
            for x in 0..image.width() {
                found += usize::from(image.pixel(x, y) == color);
            }
        }
        found
    }

    // A quad from (2,2) to (6,6) covers the pixels whose centres are inside it: columns and rows 2
    // to 5, sixteen, and none round them.
    #[test]
    fn an_opaque_quad_covers_exactly_the_pixels_whose_centres_it_holds() {
        let red = Color32::from_rgb(255, 0, 0);
        let image = renderer().render(
            &[primitive(
                quad(rect(2.0, 2.0, 6.0, 6.0), red, WHITE),
                everywhere(),
            )],
            8,
            8,
            1.0,
            Color32::TRANSPARENT,
        );
        assert_eq!(count(&image, Color::rgb(255, 0, 0)), 16);
        assert_eq!(image.pixel(2, 2), Color::rgb(255, 0, 0));
        assert_eq!(image.pixel(5, 5), Color::rgb(255, 0, 0));
        assert_eq!(image.pixel(1, 1), Color::TRANSPARENT);
        assert_eq!(image.pixel(6, 6), Color::TRANSPARENT);
        assert_eq!(image.pixel(6, 2), Color::TRANSPARENT);
    }

    // Half red, premultiplied (128, 0, 0, 128), over opaque black: 128/255 + 0 is 128 and the alpha
    // 128/255 (1 - 1) + 1 is 255. A pixel on the diagonal blended twice would be
    // 128 + 128 (1 - 128/255) = 192.
    #[test]
    fn two_triangles_sharing_an_edge_blend_each_pixel_once() {
        let half = Color32::from_rgba_premultiplied(128, 0, 0, 128);
        let image = renderer().render(
            &[primitive(
                quad(rect(0.0, 0.0, 8.0, 8.0), half, WHITE),
                everywhere(),
            )],
            8,
            8,
            1.0,
            Color32::BLACK,
        );
        assert_eq!(count(&image, Color::rgb(128, 0, 0)), 64);
    }

    // Blue at half alpha, premultiplied (0, 0, 128, 128), over opaque (200, 0, 0): red
    // 200 (1 - 128/255) = 99.6, so 100; blue 128; alpha 1.
    #[test]
    fn blending_is_premultiplied_source_over() {
        let ground = Color32::from_rgb(200, 0, 0);
        let half_blue = Color32::from_rgba_premultiplied(0, 0, 128, 128);
        let image = renderer().render(
            &[primitive(
                quad(rect(0.0, 0.0, 2.0, 2.0), half_blue, WHITE),
                everywhere(),
            )],
            2,
            2,
            1.0,
            ground,
        );
        assert_eq!(image.pixel(0, 0), Color::rgb(100, 0, 128));
        assert_eq!(count(&image, Color::rgb(100, 0, 128)), 4);
    }

    // A 2 x 2 texture drawn nearest over 4 x 4 pixels: each texel is a 2 x 2 block, the vertex
    // colour white leaving it as it is.
    #[test]
    fn a_texture_is_sampled_at_its_texels() {
        let mut renderer = renderer();
        let image_id = TextureId::User(7);
        let texels = vec![
            Color32::from_rgb(255, 0, 0),
            Color32::from_rgb(0, 255, 0),
            Color32::from_rgb(0, 0, 255),
            Color32::WHITE,
        ];
        let (id, delta) = texture(image_id, 2, texels, TextureOptions::NEAREST);
        renderer.set_texture(id, &delta);
        let image = renderer.render(
            &[primitive(
                quad(rect(0.0, 0.0, 4.0, 4.0), Color32::WHITE, image_id),
                everywhere(),
            )],
            4,
            4,
            1.0,
            Color32::BLACK,
        );
        assert_eq!(image.pixel(0, 0), Color::rgb(255, 0, 0));
        assert_eq!(image.pixel(1, 1), Color::rgb(255, 0, 0));
        assert_eq!(image.pixel(3, 0), Color::rgb(0, 255, 0));
        assert_eq!(image.pixel(0, 3), Color::rgb(0, 0, 255));
        assert_eq!(image.pixel(3, 3), Color::WHITE);
        // A patch written into it changes that texel alone.
        let patch = ImageDelta {
            image: ImageData::Color(Arc::new(ColorImage::new(
                [1, 1],
                vec![Color32::from_rgb(9, 9, 9)],
            ))),
            options: TextureOptions::NEAREST,
            pos: Some([1, 1]),
        };
        renderer.set_texture(image_id, &patch);
        let image = renderer.render(
            &[primitive(
                quad(rect(0.0, 0.0, 4.0, 4.0), Color32::WHITE, image_id),
                everywhere(),
            )],
            4,
            4,
            1.0,
            Color32::BLACK,
        );
        assert_eq!(image.pixel(3, 3), Color::rgb(9, 9, 9));
        assert_eq!(image.pixel(0, 0), Color::rgb(255, 0, 0));
    }

    // The clip from (1,1) to (3,3) lets through the pixels 1 and 2 each way, four, of a quad over
    // the whole target; a texture never delivered draws nothing.
    #[test]
    fn the_clip_rectangle_bounds_what_is_drawn() {
        let green = Color32::from_rgb(0, 255, 0);
        let renderer = renderer();
        let image = renderer.render(
            &[
                primitive(
                    quad(rect(0.0, 0.0, 6.0, 6.0), green, WHITE),
                    rect(1.0, 1.0, 3.0, 3.0),
                ),
                primitive(
                    quad(
                        rect(0.0, 0.0, 6.0, 6.0),
                        Color32::WHITE,
                        TextureId::User(99),
                    ),
                    everywhere(),
                ),
            ],
            6,
            6,
            1.0,
            Color32::BLACK,
        );
        assert_eq!(count(&image, Color::rgb(0, 255, 0)), 4);
        assert_eq!(image.pixel(1, 1), Color::rgb(0, 255, 0));
        assert_eq!(image.pixel(3, 3), Color::BLACK);
    }

    // At two pixels a point, a point square is four pixels.
    #[test]
    fn a_point_is_pixels_per_point_pixels_each_way() {
        let red = Color32::from_rgb(255, 0, 0);
        let image = renderer().render(
            &[primitive(
                quad(rect(1.0, 1.0, 2.0, 2.0), red, WHITE),
                everywhere(),
            )],
            6,
            6,
            2.0,
            Color32::TRANSPARENT,
        );
        assert_eq!(count(&image, Color::rgb(255, 0, 0)), 4);
        assert_eq!(image.pixel(2, 2), Color::rgb(255, 0, 0));
        assert_eq!(image.pixel(3, 3), Color::rgb(255, 0, 0));
        assert_eq!(image.pixel(4, 4), Color::TRANSPARENT);
        assert_eq!(image.device_pixel_ratio(), 2.0);
    }

    #[test]
    fn a_target_is_cropped_outward_to_whole_pixels_inside_the_screen() {
        let target = rect(10.0, 10.0, 60.0, 40.0);
        assert_eq!(
            crop_rect(target, 1.0, 1360, 860),
            IntRect::new(10, 10, 50, 30)
        );
        // 12.5 down to 12, 75 and 50 exactly.
        assert_eq!(
            crop_rect(target, 1.25, 1700, 1075),
            IntRect::new(12, 12, 63, 38)
        );
    }

    /// A frame of egui text in a target named "testTarget".
    fn draw_text(ui: &mut egui::Ui) {
        let shown = ui.vertical(|ui| {
            begin_target(ui, TargetSpec::new("testTarget", TargetKind::Panel, "Test"));
            ui.label("Ink from the font atlas");
        });
        end_target(ui, shown.response.rect);
    }

    // egui's frames deliver the font atlas as a texture; drawn with it, a label is ink on the
    // window's ground, and a screenshot of a target is its rectangle.
    #[test]
    fn a_headless_frame_draws_text_from_the_atlas_egui_delivered() {
        let mut headless = Headless::new();
        headless.settle_ui(draw_text).expect("the frame settles");
        let frame = headless.snapshot();
        let target = frame
            .target("testTarget")
            .expect("the target is recorded")
            .rect;
        let picture = headless
            .screenshot("testTarget")
            .expect("the target is grabbed");
        let crop = crop_rect(target, 1.0, 1360, 860);
        assert_eq!(picture.width() as i32, crop.width);
        assert_eq!(picture.height() as i32, crop.height);
        let ground = katana_window::theme::palette::window();
        let mut ink = 0;
        for y in 0..picture.height() {
            for x in 0..picture.width() {
                ink += usize::from(picture.pixel(x, y) != ground);
            }
        }
        assert!(ink > 20, "the label left {ink} pixels of ink");
        let whole = headless.render();
        assert_eq!((whole.width(), whole.height()), (1360, 860));
        assert!(headless.screenshot("noSuchTarget").is_err());
    }

    // The same frame at 1.25: the picture is the C++ window's 1700 x 1075 device pixels.
    #[test]
    fn a_headless_frame_at_a_scale_is_that_many_device_pixels() {
        let mut headless = Headless::with_pixels_per_point(1.25);
        headless.settle_ui(draw_text).expect("the frame settles");
        assert_eq!(headless.screen_pixels(), (1700, 1075));
        let whole = headless.render();
        assert_eq!((whole.width(), whole.height()), (1700, 1075));
    }
}
