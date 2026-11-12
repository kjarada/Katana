//! The twin of tests/qt_widgets/annotation/test_leader_preview.cpp: a smart leader painted as a
//! tool's preview or as a grip drags it (plan_painter's paint_plan_geometry; docs/annotation.md
//! "In the window"). Its note is read off the entity its tip is on every time it is drawn, so it
//! has to be painted with the drawing it reads. Painted with none, the preview of the preset
//! leader tools (LEADERLEVEL ...) was a bare arrow and landing while the person chose where the
//! note goes.

use std::sync::Arc;

use katana_cad::annotation::leader_presets::leader_context;
use katana_cad::document::Document;
use katana_commands::entity_commands::create_entities;
use katana_entity::{
    AnchorPoint, AnchorRef, DimensionStyle, Entity, Geometry, LeaderGeometry, PointGeometry,
    PropertyValue,
};
use katana_geometry::Point2;
use katana_window::canvas::{Color, Image, Pen};
use katana_window::plan_painter::{
    PlanFrame, PlanPaintCache, PlanPaintOptions, PlanSource, paint_plan_geometry,
};

use crate::raster::{backend, paint_on};

/// 300 x 300 pixels at 40 a unit about (50.8, 50.5): the unit square from (50, 50) to (51, 51)
/// fills a third of it, with room for a note to its right. A 2.5 mm note at 1 : 100 is 0.25 units,
/// 10 pixels.
fn preview(leader: &LeaderGeometry, source: Option<&PlanSource>) -> Image {
    let mut frame = PlanFrame::default();
    frame.transform.resize(300.0, 300.0);
    frame.transform.scale = 40.0;
    frame.transform.center = Point2::new(50.8, 50.5);
    let options = PlanPaintOptions {
        annotation_scale: 100.0,
        raster: Some(backend()),
        ..PlanPaintOptions::default()
    };
    let mut image = Image::new(300, 300);
    image.fill(Color::BLACK);
    let mut cache = PlanPaintCache::default();
    let geometry = Geometry::Leader(leader.clone());
    paint_on(&mut image, |canvas| {
        canvas.set_pen(Pen::new(Color::WHITE));
        paint_plan_geometry(
            canvas,
            &frame,
            &options,
            &mut cache,
            &geometry,
            &DimensionStyle::default(),
            source,
        );
    });
    image
}

fn differing(a: &Image, b: &Image) -> usize {
    let mut count = 0;
    for y in 0..a.height() {
        for x in 0..a.width() {
            count += usize::from(a.pixel(x, y) != b.pixel(x, y));
        }
    }
    count
}

fn inked(image: &Image) -> usize {
    let mut count = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            count += usize::from(image.pixel(x, y) != Color::rgb(0, 0, 0));
        }
    }
    count
}

fn plain_leader() -> LeaderGeometry {
    LeaderGeometry {
        vertices: vec![Point2::new(50.0, 50.0), Point2::new(51.0, 51.0)],
        text: String::from("plain"),
        ..LeaderGeometry::default()
    }
}

mod leader_preview {
    use super::*;

    #[test]
    fn a_smart_leader_is_painted_with_its_note_read_off_the_drawing_its_tip_is_on() {
        let mut document = Document::default();
        let mut point = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(50.0, 50.0),
            }),
            ..Entity::default()
        };
        point
            .properties
            .insert(String::from("elevation"), PropertyValue::Real(30.25));
        document
            .execute(create_entities(vec![point]))
            .expect("the point is made");
        let target = document.last_created_entities()[0];

        let leader = LeaderGeometry {
            vertices: vec![Point2::new(50.0, 50.0), Point2::new(51.0, 51.0)],
            tip_ref: AnchorRef {
                entity: target,
                point: AnchorPoint::Position,
                ..AnchorRef::default()
            },
            text: String::from("RL {rl:.3f}"),
            fields: true,
            ..LeaderGeometry::default()
        };

        let source = PlanSource {
            model: Some(document.model()),
            leaders: Some(Arc::new(leader_context(&document))),
            ..PlanSource::default()
        };

        // With no drawing the note's one line names a value nothing has, and is dropped: the
        // arrow, the line and the landing. With it, "RL 30.250" as well - nine glyphs of ten
        // pixels, a hundred pixels of ink and more.
        let bare = preview(&leader, None);
        let read = preview(&leader, Some(&source));
        assert!(inked(&bare) > 0, "the leader itself was not drawn");
        assert!(
            inked(&read) > inked(&bare) + 60,
            "bare {}, read {}",
            inked(&bare),
            inked(&read)
        );
        assert!(differing(&read, &bare) > 60);
    }

    #[test]
    fn a_plain_leader_or_one_whose_target_is_gone_paints_as_it_did_with_or_without_the_drawing() {
        let document = Document::default();
        let source = PlanSource {
            model: Some(document.model()),
            leaders: Some(Arc::new(leader_context(&document))),
            ..PlanSource::default()
        };

        // A plain leader says its own text, drawing or no drawing.
        let plain = plain_leader();
        assert_eq!(
            differing(&preview(&plain, None), &preview(&plain, Some(&source))),
            0
        );
        assert!(inked(&preview(&plain, Some(&source))) > 0);

        // A smart one whose target is not in the drawing has nothing to read: the arrow alone,
        // as with no drawing at all.
        let orphan = LeaderGeometry {
            tip_ref: AnchorRef {
                entity: 9999,
                point: AnchorPoint::Position,
                ..AnchorRef::default()
            },
            text: String::from("RL {rl:.3f}"),
            fields: true,
            ..plain
        };
        assert_eq!(
            differing(&preview(&orphan, None), &preview(&orphan, Some(&source))),
            0
        );
    }
}
