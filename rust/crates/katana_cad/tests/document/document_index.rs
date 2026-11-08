// The Document's spatial index after BULK changes (tests/cad/test_document_index.cpp).
//
// The index is kept in step with the model incrementally, one entity at a
// time, which is right for a click and wrong for an import: an index that
// starts empty keeps its default one-unit cell, so a 12 km survey imported
// into it was filed into 722,715 buckets with 4,889 boxes on the oversized
// list every query scans, the import's execute took over a second, and
// queries stayed 36x slower until the project was reopened - because opening
// is what rebuilt the index and chose a cell size from the data.
//
// The rules these tests pin down (Document::apply_to_spatial_index):
//   * a drawing that has grown to twice, or shrunk to half, the size its cell
//     was chosen for is rebuilt - which includes the empty drawing, whose
//     cell was chosen for none;
//   * so is one whose oversized list has grown past twice what the last
//     rebuild left plus a hundredth of the drawing;
//   * anything else is incremental - the ordinary click, and a command that
//     touches many entities without changing what the cell was chosen for.
//
// The cell sizes below are worked out by hand from SpatialIndex's rule: the
// cell is twice the mean box side, (sum of width + height) / (2 n) * 2.

use katana_cad::Document;
use katana_cad::spatial_query::query_extents;
use katana_commands::{
    CommandPtr, EntityAttributes, Transaction, create_entities, create_line, scale_entities,
};
use katana_entity::{Entity, Geometry, PointGeometry};
use katana_geometry::{Box2, Point2, Polyline2, Segment2, SpatialId};

/// `count` horizontal lines of `length`, one above the other, 1 unit apart.
/// Each box is `length` wide and 0 high, so it adds exactly `length` to the
/// sum the cell size is taken from.
fn horizontal_lines(count: i32, length: f64, y0: f64) -> Vec<Entity> {
    (0..count)
        .map(|i| {
            let y = y0 + f64::from(i);
            Entity {
                geometry: Geometry::Line(Segment2::new(
                    Point2::new(0.0, y),
                    Point2::new(length, y),
                )),
                ..Entity::default()
            }
        })
        .collect()
}

/// What a file import executes: one transaction holding one bulk create.
fn import(document: &mut Document, entities: Vec<Entity>) {
    let mut transaction = Transaction::new("IMPORT");
    transaction.add(create_entities(entities));
    if let Err(error) = document.execute(Box::new(transaction)) {
        panic!("{}", error.describe());
    }
}

fn line(start: Point2, end: Point2) -> CommandPtr {
    create_line(start, end, EntityAttributes::default())
}

mod document_index {
    use super::*;

    #[test]
    fn an_import_into_an_empty_drawing_chooses_the_cell_size_from_what_it_imported() {
        let mut document = Document::default();
        // (0,0)-(10,0) has a box 10 x 0 and (0,0)-(0,30) one 0 x 30: sum of
        // sides 10 + 30 = 40 over 2 boxes, mean side 40 / (2 * 2) = 10, cell
        // 2 * 10 = 20. Inserted one at a time, the index kept the default 1.
        let entities = vec![
            Entity {
                geometry: Geometry::Line(Segment2::new(
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 0.0),
                )),
                ..Entity::default()
            },
            Entity {
                geometry: Geometry::Line(Segment2::new(
                    Point2::new(0.0, 0.0),
                    Point2::new(0.0, 30.0),
                )),
                ..Entity::default()
            },
        ];
        import(&mut document, entities);

        assert_eq!(document.spatial_index().cell_size(), 20.0);
        assert_eq!(document.spatial_index().size(), 2);
    }

    #[test]
    fn scaling_every_entity_keeps_the_cell_until_the_boxes_no_longer_fit_it() {
        let mut document = Document::default();
        // 100 lines 10 long: mean side 100 * 10 / (2 * 100) = 5, cell 10.
        import(&mut document, horizontal_lines(100, 10.0, 0.0));
        assert_eq!(document.spatial_index().cell_size(), 10.0);
        let all = document.model().entities.ids();

        // Every entity scaled by 2 about the origin: each line is now 20 long,
        // 2 cells, and fits the grid as it is. A rebuild would have chosen 20
        // (mean side 100 * 20 / 200 = 10); the cell is still 10, so there was
        // none - the command touched the whole drawing and changed no cell
        // choice worth an O(n) rebuild.
        assert!(
            document
                .execute(scale_entities(all.clone(), Point2::new(0.0, 0.0), 2.0))
                .is_ok()
        );
        assert_eq!(document.spatial_index().cell_size(), 10.0);
        assert_eq!(document.spatial_index().oversized_count(), 0);

        // By 1000 more: each line is 20,000 long, 2,001 cells of 10 - past the
        // 1,024 a box may cover - so all 100 go on the oversized list, which is
        // over 2 * 0 + 100 / 100 = 1. Rebuilt: mean side 100 * 20000 / 200 =
        // 10000, cell 20000, and nothing is oversized any more.
        assert!(
            document
                .execute(scale_entities(all, Point2::new(0.0, 0.0), 1000.0))
                .is_ok()
        );
        assert_eq!(document.spatial_index().cell_size(), 20000.0);
        assert_eq!(document.spatial_index().oversized_count(), 0);
        assert_eq!(document.spatial_index().size(), 100);
    }

    #[test]
    fn boxes_too_big_for_the_cell_rebuild_it_once_they_are_more_than_a_hundredth_of_the_drawing() {
        let mut document = Document::default();
        // 100 lines 10 long: cell 10, nothing oversized.
        import(&mut document, horizontal_lines(100, 10.0, 0.0));
        assert_eq!(document.spatial_index().cell_size(), 10.0);

        // A line 19,900 long is 1,991 cells of 10, past the 1,024 a box may
        // cover: oversized. One of them is not more than 2 * 0 + 101 / 100 = 1.
        assert!(
            document
                .execute(line(Point2::new(0.0, 500.0), Point2::new(19900.0, 500.0)))
                .is_ok()
        );
        assert_eq!(document.spatial_index().oversized_count(), 1);
        assert_eq!(document.spatial_index().cell_size(), 10.0);

        // Two are more than 2 * 0 + 102 / 100 = 1: rebuilt. Sum of sides
        // 100 * 10 + 2 * 19900 = 40800 over 102 boxes, mean 40800 / 204 = 200,
        // cell 400 - where a 19,900 line is 50 cells and fits.
        assert!(
            document
                .execute(line(Point2::new(0.0, 600.0), Point2::new(19900.0, 600.0)))
                .is_ok()
        );
        assert_eq!(document.spatial_index().cell_size(), 400.0);
        assert_eq!(document.spatial_index().oversized_count(), 0);
        assert_eq!(document.spatial_index().size(), 102);
    }

    #[test]
    fn a_drawing_that_doubles_one_entity_at_a_time_has_its_cell_chosen_again() {
        let mut document = Document::default();
        // 10 lines 10 long: cell 10, chosen for 10 entities.
        import(&mut document, horizontal_lines(10, 10.0, 0.0));
        assert_eq!(document.spatial_index().cell_size(), 10.0);

        // Lines 30 long, one command each. The k-th leaves 10 + k entities,
        // and 1 is never a tenth of that, so only the doubling rule can fire:
        // at 20.
        for k in 1..=9 {
            let y = 100.0 + f64::from(k);
            assert!(
                document
                    .execute(line(Point2::new(0.0, y), Point2::new(30.0, y)))
                    .is_ok()
            );
        }
        assert_eq!(
            document.spatial_index().cell_size(),
            10.0,
            "19 entities: still incremental"
        );

        assert!(
            document
                .execute(line(Point2::new(0.0, 110.0), Point2::new(30.0, 110.0)))
                .is_ok()
        );
        // Sum of sides 10 * 10 + 10 * 30 = 400 over 20 boxes, mean
        // 400 / 40 = 10, cell 20.
        assert_eq!(
            document.spatial_index().cell_size(),
            20.0,
            "20 entities: rebuilt"
        );
        assert_eq!(document.spatial_index().size(), 20);
    }

    #[test]
    fn one_line_drawn_into_a_large_drawing_is_inserted_not_rebuilt() {
        let mut document = Document::default();
        import(&mut document, horizontal_lines(1000, 10.0, 0.0));
        assert_eq!(document.spatial_index().cell_size(), 10.0);

        // A line 1000 long would move the mean if the index were rebuilt:
        // (1000 * 10 + 1000) / (2 * 1001) is about 5.49, cell about 10.99.
        assert!(
            document
                .execute(line(Point2::new(0.0, 5000.0), Point2::new(1000.0, 5000.0)))
                .is_ok()
        );
        assert_eq!(document.spatial_index().cell_size(), 10.0);
        assert_eq!(document.spatial_index().size(), 1001);
        // Inserted, and found.
        let found = document.spatial_index().query(Box2::new(
            Point2::new(499.0, 4999.0),
            Point2::new(501.0, 5001.0),
        ));
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn undoing_an_import_chooses_the_cell_again_for_what_remains() {
        let mut document = Document::default();
        import(&mut document, horizontal_lines(100, 10.0, 0.0));
        assert_eq!(document.spatial_index().cell_size(), 10.0);

        // 100 lines 1000 long: sum 100 * 10 + 100 * 1000 = 101000 over 200
        // boxes, mean 101000 / 400 = 252.5, cell 505.
        import(&mut document, horizontal_lines(100, 1000.0, 200.0));
        assert_eq!(document.spatial_index().cell_size(), 505.0);

        // The undo removes 100 of 200: back to the first 100, cell 10 again.
        assert!(document.undo().is_ok());
        assert_eq!(document.spatial_index().cell_size(), 10.0);
        assert_eq!(document.spatial_index().size(), 100);
        assert!(document.redo().is_ok());
        assert_eq!(document.spatial_index().cell_size(), 505.0);
        assert_eq!(document.spatial_index().size(), 200);
    }

    #[test]
    fn an_imported_drawing_has_the_same_index_as_the_same_drawing_reopened() {
        // A survey-like mix: points, short strings and a few long ones spread
        // over 2 km. Inserted one by one into the default cell, the long
        // strings are what overflowed onto the oversized list.
        let mut entities = Vec::new();
        for i in 0..3000 {
            let t = f64::from(i);
            let x = (t * 37.0) % 2000.0;
            let y = (t * 91.0) % 1500.0;
            let geometry = if i % 2 == 0 {
                Geometry::Point(PointGeometry {
                    position: Point2::new(x, y),
                })
            } else {
                let step = (0.25 + (t * 0.731) % 4.0) * if i % 32 == 1 { 25.0 } else { 1.0 };
                let vertices = (0..12)
                    .map(|v| {
                        let v = f64::from(v);
                        Point2::new(x + v * step, y + (t + v).sin() * step * 0.5)
                    })
                    .collect();
                Geometry::Polyline(Polyline2::new(vertices, false))
            };
            entities.push(Entity {
                geometry,
                ..Entity::default()
            });
        }

        // The test's own directory: the tests of a binary run at once.
        let parent = std::env::temp_dir().join(format!(
            "katana-cad-tests-document-index-{}",
            std::process::id()
        ));
        let directory = parent.join("imported.katana");
        let _ = std::fs::remove_dir_all(&parent);

        // Scoped: an open project holds its database file until its Document
        // goes, and Windows will not remove a file that is open.
        {
            let mut imported = Document::default();
            import(&mut imported, entities);
            assert!(imported.save_as(&directory).is_ok());

            let mut reopened = Document::default();
            let opened = reopened.open(&directory);
            assert!(opened.is_ok(), "{opened:?}");

            let a = imported.spatial_index();
            let b = reopened.spatial_index();
            // Exactly, not nearly: both come from one rebuild over the same
            // boxes in the same (ascending id) order, and the cell size is a
            // sum over them.
            assert_eq!(a.cell_size(), b.cell_size());
            assert_eq!(a.oversized_count(), b.oversized_count());
            assert_eq!(a.bucket_count(), b.bucket_count());
            assert_eq!(a.size(), b.size());
            assert_eq!(a.size(), 3000);
            assert!(
                a.cell_size() > 1.0,
                "the cell was chosen from the data, not left at its default"
            );

            // And it answers as a scan does.
            let window = Box2::new(Point2::new(900.0, 700.0), Point2::new(1100.0, 800.0));
            let mut scanned: Vec<SpatialId> = Vec::new();
            imported.model().entities.for_each(|entity| {
                if query_extents(imported.model(), entity).intersects(window) {
                    scanned.push(entity.id);
                }
            });
            assert_eq!(a.query(window), scanned);
            assert!(!scanned.is_empty());
        }

        let _ = std::fs::remove_dir_all(&parent);
    }
}
