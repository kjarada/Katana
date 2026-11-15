//! The twin of tests/qt_widgets/annotation/test_annotation_ui.cpp's AnnotationPaint suite: the
//! plan painter's paper sizes and marks for the annotation system (docs/annotation.md, "In the
//! window"). The file's AnnotationManagers cases are AN's and V3's.

use katana_cad::plot::PlotSettings;
use katana_entity::{
    Color as EntityColor, Entity, Geometry, LabelGeometry, LabelStyle, Model, PointGeometry,
    PropertyValue, TextGeometry, TextJustify, TextStyle,
};
use katana_geometry::{Point2, Segment2};
use katana_window::canvas::{Color, Image};
use katana_window::plan_painter::{
    PlanFrame, PlanMedium, PlanPaintCache, PlanPaintOptions, PlanPaintStats, PlanSource, paint_plan,
};

use crate::raster::{backend, paint_on};

const WHITE: Color = Color::WHITE;

/// qGray.
fn grey(c: Color) -> i32 {
    (i32::from(c.r) * 11 + i32::from(c.g) * 16 + i32::from(c.b) * 5) / 32
}

/// A frame on paper at `pixels_per_millimetre`, drawing 1 : `scale` about `centre`: a millimetre
/// of paper is scale / 1000 model units.
fn paper_frame(pixels_per_millimetre: f64, scale: f64, centre: Point2) -> PlanFrame<'static> {
    let mut frame = PlanFrame::default();
    frame.transform.resize(400.0, 400.0);
    frame.transform.scale = pixels_per_millimetre * 1000.0 / scale;
    frame.transform.center = centre;
    frame
}

fn painted_on_paper(model: &Model, scale: f64, centre: Point2) -> (Image, PlanPaintStats) {
    let settings = PlotSettings::default();
    let options = PlanPaintOptions {
        medium: PlanMedium::Paper,
        pixels_per_millimetre: 4.0,
        plot: Some(&settings),
        annotation_scale: scale,
        raster: Some(backend()),
        ..PlanPaintOptions::default()
    };
    let frame = paper_frame(4.0, scale, centre);
    let mut image = Image::new(400, 400);
    image.fill(WHITE);
    let source = PlanSource {
        model: Some(model),
        ..PlanSource::default()
    };
    let mut cache = PlanPaintCache::default();
    let mut stats = PlanPaintStats::default();
    paint_on(&mut image, |canvas| {
        stats = paint_plan(canvas, &source, &frame, &options, &mut cache);
    });
    (image, stats)
}

/// The rows from the first to the last with any ink: the height of what was drawn, in pixels.
fn ink_height(image: &Image) -> i32 {
    let mut first = -1;
    let mut last = -1;
    for y in 0..image.height() {
        if (0..image.width()).any(|x| image.pixel(x, y) != WHITE) {
            if first < 0 {
                first = y as i32;
            }
            last = y as i32;
        }
    }
    if first < 0 { 0 } else { last - first + 1 }
}

fn dark_pixels(image: &Image) -> i32 {
    let mut count = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            count += i32::from(grey(image.pixel(x, y)) < 80);
        }
    }
    count
}

/// Pixels with any visible ink, antialiased edges included: small text is mostly grey edge, and
/// how many of its pixels come out nearly black depends on the platform's fonts (Arial on Windows
/// gave 10 where Linux gave more).
fn ink_pixels(image: &Image) -> i32 {
    let mut count = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            count += i32::from(grey(image.pixel(x, y)) < 200);
        }
    }
    count
}

fn model_with_styled_text(text: &str, paper_height: f64) -> Model {
    let mut model = Model::new();
    model
        .text_styles
        .add(TextStyle {
            name: String::from("Notes"),
            paper_height,
            ..TextStyle::default()
        })
        .expect("the text style is added");
    model
        .entities
        .add(Entity {
            geometry: Geometry::Text(TextGeometry {
                position: Point2::new(0.0, 0.0),
                text: String::from(text),
                height: 1.0,
                rotation: 0.0,
                style: String::from("Notes"),
                justify: TextJustify::MiddleCentre,
                ..TextGeometry::default()
            }),
            ..Entity::default()
        })
        .expect("the text is added");
    model
}

mod annotation_paint {
    use super::*;

    #[test]
    fn a_paper_sized_text_plots_the_same_height_at_every_scale() {
        // 5 mm at 4 px/mm is 20 px of em; the glyphs' ink is a fixed part of it whatever the
        // scale. A model-unit text of 1 m would be 20 px at 1:200 and 2 px at 1:2000.
        let model = model_with_styled_text("HHHH", 5.0);
        let at200 = ink_height(&painted_on_paper(&model, 200.0, Point2::new(0.0, 0.0)).0);
        let at2000 = ink_height(&painted_on_paper(&model, 2000.0, Point2::new(0.0, 0.0)).0);
        assert!(at200 > 5, "{at200}");
        assert!(
            (at200 - at2000).abs() <= 1,
            "1:200 {at200} px, 1:2000 {at2000} px"
        );

        let mut legacy = Model::new();
        legacy
            .entities
            .add(Entity {
                geometry: Geometry::Text(TextGeometry {
                    position: Point2::new(0.0, 0.0),
                    text: String::from("HHHH"),
                    height: 1.0,
                    rotation: 0.0,
                    ..TextGeometry::default()
                }),
                ..Entity::default()
            })
            .expect("the text is added");
        let legacy200 = ink_height(&painted_on_paper(&legacy, 200.0, Point2::new(2.0, 0.5)).0);
        let legacy2000 = ink_height(&painted_on_paper(&legacy, 2000.0, Point2::new(2.0, 0.5)).0);
        assert!(
            legacy200 > 3 * legacy2000,
            "a model-unit text shrinks with the scale, as before"
        );
    }

    #[test]
    fn a_white_text_style_prints_black() {
        let mut model = model_with_styled_text("HHHH", 5.0);
        let mut white = model
            .text_styles
            .find("Notes")
            .expect("the style is there")
            .clone();
        white.color = Some(EntityColor {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        });
        model
            .text_styles
            .update(&white)
            .expect("the style is updated");
        assert!(
            dark_pixels(&painted_on_paper(&model, 500.0, Point2::new(0.0, 0.0)).0) > 20,
            "white on white paper would vanish: it prints black (D7)"
        );
    }

    #[test]
    fn a_mask_hides_the_line_under_the_text() {
        let mut model = Model::new();
        model
            .entities
            .add(Entity {
                geometry: Geometry::Line(Segment2::new(
                    Point2::new(-20.0, 0.0),
                    Point2::new(20.0, 0.0),
                )),
                ..Entity::default()
            })
            .expect("the line is added");
        model
            .text_styles
            .add(TextStyle {
                name: String::from("Masked"),
                paper_height: 5.0,
                mask: true,
                ..TextStyle::default()
            })
            .expect("the text style is added");
        // A text of spaces: nothing but its mask, which interrupts the line.
        model
            .entities
            .add(Entity {
                geometry: Geometry::Text(TextGeometry {
                    position: Point2::new(0.0, 0.0),
                    text: String::from("      "),
                    height: 1.0,
                    rotation: 0.0,
                    style: String::from("Masked"),
                    justify: TextJustify::MiddleCentre,
                    ..TextGeometry::default()
                }),
                ..Entity::default()
            })
            .expect("the text is added");
        let (image, _) = painted_on_paper(&model, 1000.0, Point2::new(0.0, 0.0));
        // Along the line's row: ink, then the mask's gap, then ink again.
        let row = 200;
        let mut runs = 0;
        let mut in_run = false;
        for x in 0..image.width() {
            let inked = grey(image.pixel(x, row)) < 200;
            runs += i32::from(inked && !in_run);
            in_run = inked;
        }
        assert_eq!(runs, 2);
    }

    #[test]
    fn labels_are_placed_and_counted_by_the_painter() {
        let mut model = Model::new();
        model
            .label_styles
            .add(LabelStyle {
                name: String::from("pt"),
                text: String::from("{point}"),
                ..LabelStyle::default()
            })
            .expect("the label style is added");
        let mut point = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(0.0, 0.0),
            }),
            ..Entity::default()
        };
        point.properties.insert(
            String::from("point"),
            PropertyValue::Text(String::from("101")),
        );
        model.entities.add(point).expect("the point is added");
        let target = model.entities.next_id() - 1;
        // The point on its own, for the ink the label adds to it.
        let point_ink = ink_pixels(&painted_on_paper(&model, 500.0, Point2::new(0.0, 0.0)).0);
        model
            .entities
            .add(Entity {
                geometry: Geometry::Label(LabelGeometry {
                    target,
                    style: String::from("pt"),
                    anchor: Point2::new(0.0, 0.0),
                    ..LabelGeometry::default()
                }),
                ..Entity::default()
            })
            .expect("the label is added");
        let (image, stats) = painted_on_paper(&model, 500.0, Point2::new(0.0, 0.0));
        assert_eq!(stats.labels_placed, 1);
        assert_eq!(stats.labels_suppressed, 0);
        // "101" at 2.5 mm, 4 px a millimetre: tens of pixels of ink over the point's own mark,
        // whatever the platform's font.
        assert!(
            ink_pixels(&image) > point_ink + 20,
            "point alone {point_ink} px of ink"
        );
    }
}
