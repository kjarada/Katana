//! tests/interop/test_interop.cpp: import and export.
//!
//! These tests write real files through GDAL and read them back, because the
//! whole point of this layer is the boundary with a third-party library: a
//! mock would test only our own assumptions about how GDAL behaves, which is
//! exactly the thing that needs checking.

use std::path::Path;

use katana_commands::{CommandStack, Transaction, create_entities, create_layer};
use katana_core::ErrorCode;
use katana_entity::entity::{ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, heights_of, set_heights};
use katana_entity::{
    Entity, EntityId, Geometry, Layer, Model, PointGeometry, PropertyValue, TextGeometry,
};
use katana_geometry::{Box2, Circle2, Point2, Polyline2, Segment2, Vec2};
use katana_gis::formats::find_format;
use katana_interop::export::{VectorExportOptions, export_vector};
use katana_interop::import::{
    SourceKind, VectorImportOptions, VectorImportResult, advise_placement, import_point_cloud,
    import_raster, import_vector, kind_for_path,
};
use katana_interop::reference_data::{
    PointColorMode, RasterOverlay, ReferenceData, classification_color, color_for_point,
};
use katana_math::numerics::{PI, TWO_PI};
use katana_pointcloud::PointCloudPoint;

use crate::{TempDir, double_eq, file_text, must, near, refused, warned, write_file};

fn temp_dir(name: &str) -> TempDir {
    TempDir::new("katana-interop-", name)
}

fn model_with(entities: Vec<Entity>) -> Model {
    let mut model = Model::default();
    for entity in entities {
        must(model.entities.add(entity));
    }
    model
}

fn closed_square(size: f64) -> Entity {
    Entity {
        geometry: Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(size, 0.0),
                Point2::new(size, size),
                Point2::new(0.0, size),
            ],
            true,
        )),
        ..Entity::default()
    }
}

fn segment(start: Point2, end: Point2) -> Geometry {
    Geometry::Line(Segment2::new(start, end))
}

fn point_at(x: f64, y: f64) -> Geometry {
    Geometry::Point(PointGeometry {
        position: Point2::new(x, y),
    })
}

fn export(model: &Model, path: &Path) -> katana_core::Result<katana_interop::VectorExportResult> {
    export_vector(model, path, &VectorExportOptions::default())
}

fn import(path: &Path) -> katana_core::Result<VectorImportResult> {
    import_vector(path, &VectorImportOptions::default())
}

// ---- routing ------------------------------------------------------------------------------------

mod interop_routing {
    use super::*;

    #[test]
    fn extensions_are_classified() {
        let kind = |name: &str| kind_for_path(Path::new(name));
        assert_eq!(kind("parcels.shp"), SourceKind::Vector);
        // Case insensitive.
        assert_eq!(kind("parcels.GeoJSON"), SourceKind::Vector);
        assert_eq!(kind("site.gpkg"), SourceKind::Vector);
        assert_eq!(kind("ortho.tif"), SourceKind::Raster);
        assert_eq!(kind("terrain.asc"), SourceKind::Raster);
        assert_eq!(kind("scan.las"), SourceKind::PointCloud);
        assert_eq!(kind("scan.laz"), SourceKind::PointCloud);
        // A .dwg is read only by GDAL's CAD driver (libopencad), which some
        // builds have (conda-forge's) and others lack (MSYS2's); the routing
        // follows GDAL's registry, so what it says follows the driver.
        assert_eq!(
            kind("drawing.dwg"),
            if find_format("CAD").is_some() {
                SourceKind::Vector
            } else {
                SourceKind::Unknown
            }
        );
        assert_eq!(kind("drawing.nosuchformat"), SourceKind::Unknown);
        assert_eq!(kind("noextension"), SourceKind::Unknown);

        // GDAL and PDAL both claim .ply. In a survey tool it is a point
        // cloud, and the routing must be deterministic rather than depending
        // on list order.
        assert_eq!(kind("mesh.ply"), SourceKind::PointCloud);
    }

    #[test]
    fn missing_file_is_not_found_rather_than_a_crash() {
        let vector = refused(
            import(Path::new("no-such-file.geojson")),
            "a missing vector",
        );
        assert_eq!(vector.code, ErrorCode::NotFound);

        let raster = refused(
            import_raster(Path::new("no-such-file.tif"), &Default::default()),
            "a missing raster",
        );
        assert_eq!(raster.code, ErrorCode::NotFound);

        let cloud = refused(
            import_point_cloud(Path::new("no-such-file.las"), &Default::default()),
            "a missing cloud",
        );
        assert_eq!(cloud.code, ErrorCode::NotFound);
    }
}

// ---- reference data -----------------------------------------------------------------------------

mod interop_reference_data {
    use super::*;

    #[test]
    fn geotransform_is_applied_through_all_four_corners() {
        // North-up: origin at the TOP-left and a negative y step, which is
        // how every georeferenced image is stored. Taking only two corners
        // would produce an inverted, empty box.
        let raster = RasterOverlay {
            width: 100,
            height: 50,
            geotransform: [1000.0, 2.0, 0.0, 5000.0, 0.0, -2.0],
            has_geotransform: true,
            ..RasterOverlay::default()
        };

        let bounds = raster.world_bounds();
        assert!(!bounds.empty());
        double_eq(bounds.min.x, 1000.0);
        double_eq(bounds.max.x, 1200.0);
        double_eq(bounds.min.y, 4900.0); // 5000 - 50*2
        double_eq(bounds.max.y, 5000.0);

        // A rotated geotransform: the bounding box must cover the rotated
        // rectangle, which a two-corner implementation cannot do.
        let rotated = RasterOverlay {
            // Swaps the axes.
            geotransform: [0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            ..raster.clone()
        };
        let rotated_bounds = rotated.world_bounds();
        double_eq(rotated_bounds.min.x, 0.0);
        double_eq(rotated_bounds.max.x, 50.0);
        double_eq(rotated_bounds.max.y, 100.0);
    }

    #[test]
    fn ids_are_never_reused() {
        let mut data = ReferenceData::default();
        let raster = RasterOverlay {
            width: 4,
            height: 4,
            ..RasterOverlay::default()
        };
        let first = data.add(raster.clone());
        let second = data.add(raster.clone());
        assert_ne!(first, second);

        assert!(data.remove(first));
        let third = data.add(raster.clone());
        assert_ne!(third, first, "a removed id must never come back");
        assert_ne!(third, second);

        data.clear();
        assert!(data.empty());
        assert_ne!(
            data.add(raster),
            third,
            "clear() must not reset the id counter either"
        );
    }

    #[test]
    fn visible_bounds_ignores_hidden_layers() {
        let mut data = ReferenceData::default();
        let visible = RasterOverlay {
            width: 10,
            height: 10,
            geotransform: [0.0, 1.0, 0.0, 10.0, 0.0, -1.0],
            has_geotransform: true,
            ..RasterOverlay::default()
        };
        data.add(visible.clone());

        let hidden = RasterOverlay {
            visible: false,
            geotransform: [1000.0, 1.0, 0.0, 1000.0, 0.0, -1.0],
            ..visible
        };
        data.add(hidden);

        let bounds = data.visible_bounds();
        assert!(!bounds.empty());
        double_eq(bounds.max.x, 10.0);
    }

    #[test]
    fn color_ramp_handles_a_degenerate_range() {
        let mut point = PointCloudPoint {
            z: 42.0,
            ..PointCloudPoint::default()
        };
        // A perfectly flat surface: every point has the same value, so the
        // range is zero and the naive normalisation would divide by it.
        let flat = color_for_point(&point, PointColorMode::Elevation, 42.0, 42.0);
        assert!(f64::from(flat.r).is_finite());
        let same = color_for_point(&point, PointColorMode::Elevation, 42.0, 42.0);
        assert_eq!(flat.r, same.r);
        assert_eq!(flat.g, same.g);
        assert_eq!(flat.b, same.b);

        // Ground and building are standard classes and must not collide.
        assert_ne!(classification_color(2).r, classification_color(6).r);
        // An unassigned class gets neutral grey rather than an invented
        // meaning.
        let unknown = classification_color(200);
        assert_eq!(unknown.r, unknown.g);
        assert_eq!(unknown.g, unknown.b);

        // SourceColor falls back to the ramp when the file has no colour,
        // rather than painting everything black.
        point.has_color = false;
        let fallback = color_for_point(&point, PointColorMode::SourceColor, 0.0, 100.0);
        let ramp = color_for_point(&point, PointColorMode::Elevation, 0.0, 100.0);
        assert_eq!(fallback.r, ramp.r);
        assert_eq!(fallback.b, ramp.b);
    }
}

// ---- export -------------------------------------------------------------------------------------

/// One entity of a DXF file's ENTITIES section: its type, and the group 70
/// flags where it has them (bit 1 of an LWPOLYLINE's is "closed").
struct DxfEntity {
    r#type: String,
    flags: i32,
}

/// Read by hand rather than through GDAL: the question is what a CAD program
/// finds in the file, and GDAL's reader would answer with its own reading of
/// it.
fn dxf_entities(path: &Path) -> Vec<DxfEntity> {
    let text = file_text(path);
    let lines: Vec<&str> = text
        .lines()
        .map(|line| line.trim_end_matches(['\r', ' ']).trim_start_matches(' '))
        .collect();
    let mut entities: Vec<DxfEntity> = Vec::new();
    let mut in_entities = false;
    let mut i = 0;
    while i + 1 < lines.len() {
        let code = lines[i];
        let value = lines[i + 1];
        if code == "2" && value == "ENTITIES" {
            in_entities = true;
        } else if in_entities && code == "0" && value == "ENDSEC" {
            break;
        } else if in_entities && code == "0" {
            entities.push(DxfEntity {
                r#type: String::from(value),
                flags: 0,
            });
        } else if in_entities
            && code == "70"
            && let Some(last) = entities.last_mut()
        {
            last.flags = value.parse().expect("group 70 is an integer");
        }
        i += 2;
    }
    entities
}

mod interop_export {
    use super::*;

    #[test]
    fn unknown_extension_is_rejected_by_name() {
        let model = model_with(vec![closed_square(10.0)]);
        let error = refused(
            export(&model, Path::new("out.xyzzy")),
            "an unknown extension",
        );
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(
            error.message.contains("xyzzy"),
            "the error should name the extension: {}",
            error.message
        );
    }

    #[test]
    fn round_trips_through_geo_json() {
        let dir = temp_dir("roundtrip");
        let path = dir.file("shapes.geojson");

        let line = Entity {
            geometry: segment(Point2::new(0.0, 0.0), Point2::new(30.0, 40.0)),
            ..Entity::default()
        };
        let point = Entity {
            geometry: point_at(5.0, 7.0),
            ..Entity::default()
        };
        let model = model_with(vec![closed_square(20.0), line, point]);

        let written = must(export(&model, &path));
        assert_eq!(written.features_written, 3);
        assert!(path.exists());

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 3);

        // The square keeps its area, and the line its length, through the
        // round trip.
        let mut saw_square = false;
        let mut saw_line = false;
        let mut saw_point = false;
        for entity in &read.entities {
            match &entity.geometry {
                Geometry::Polyline(polyline) => {
                    assert!(polyline.closed);
                    near(polyline.area().abs(), 400.0, 1e-9);
                    saw_square = true;
                }
                Geometry::Line(line) => {
                    near(line.length(), 50.0, 1e-9);
                    saw_line = true;
                }
                Geometry::Point(_) => saw_point = true,
                _ => {}
            }
        }
        assert!(saw_square);
        assert!(saw_line);
        assert!(saw_point);
    }

    #[test]
    fn a_drawing_can_be_written_to_dxf_and_keeps_its_layers() {
        // DXF has a fixed set of fields and refuses any other, and the
        // exporter used to treat "could not create field katana_id" as fatal -
        // so the one format a CAD program cannot do without could not be
        // written at all. Found by running the bundled CLI, not by a test,
        // because every export test used a format that accepts arbitrary
        // fields.
        let dir = temp_dir("dxf");
        let path = dir.file("drawing.dxf");

        let mut kerb = closed_square(20.0);
        kerb.layer = String::from("KERB");
        let line = Entity {
            geometry: segment(Point2::new(0.0, 0.0), Point2::new(30.0, 40.0)),
            layer: String::from("BOUNDARY"),
            ..Entity::default()
        };
        let mut model = model_with(Vec::new());
        for name in ["KERB", "BOUNDARY"] {
            must(model.layers.add(Layer {
                name: String::from(name),
                ..Layer::default()
            }));
        }
        // add(), not insert(): add assigns the id, insert expects one
        // already.
        must(model.entities.add(kerb));
        must(model.entities.add(line));

        let written = must(export(&model, &path));
        assert_eq!(written.features_written, 2);

        // What DXF cannot hold is SAID, not silently dropped.
        assert!(
            warned(&written.warnings, "fixed set of fields"),
            "the loss of ids and properties went unreported"
        );

        // And what it CAN hold survives: geometry, and the layer each entity
        // is on - our `layer` attribute lands in DXF's own Layer field. No
        // option needed: the importer's default layer attribute matches DXF's
        // `Layer` field, so the CAD layers come back rather than one
        // "entities".
        let read = must(import(&path));
        assert_eq!(read.entities.len(), 2);
        let mut saw_kerb = false;
        let mut saw_boundary = false;
        for entity in &read.entities {
            match &entity.geometry {
                Geometry::Polyline(polyline) => {
                    near(polyline.area().abs(), 400.0, 1e-9);
                    saw_kerb = saw_kerb || entity.layer == "KERB";
                }
                Geometry::Line(line) => {
                    near(line.length(), 50.0, 1e-9);
                    saw_boundary = saw_boundary || entity.layer == "BOUNDARY";
                }
                _ => {}
            }
        }
        assert!(saw_kerb, "the square lost its layer");
        assert!(saw_boundary, "the line lost its layer");
        // Both layers are reported as needed, and nothing asks for an
        // "entities" layer that no entity is on.
        assert!(read.layers_needed.iter().any(|layer| layer == "KERB"));
        assert!(read.layers_needed.iter().any(|layer| layer == "BOUNDARY"));
        assert!(!read.layers_needed.iter().any(|layer| layer == "entities"));

        // An explicit target layer still wins: "put everything here" means
        // that.
        let everything_here = VectorImportOptions {
            target_layer: String::from("IMPORTED"),
            ..VectorImportOptions::default()
        };
        let flattened = must(import_vector(&path, &everything_here));
        for entity in &flattened.entities {
            assert_eq!(entity.layer, "IMPORTED");
        }
    }

    #[test]
    fn closed_polylines_and_circles_go_to_dxf_as_closed_polylines_not_solid_hatches() {
        // GDAL's DXF writer turns a polygon into a HATCH with a SOLID fill by
        // default, and the exporter hands it closed polylines and circles as
        // polygons - so every parcel reached a CAD program as a filled shape.
        let dir = temp_dir("dxf-closed");
        let path = dir.file("parcels.dxf");

        let parcel = closed_square(20.0);
        let tree = Entity {
            geometry: Geometry::Circle(Circle2::new(Point2::new(50.0, 50.0), 5.0)),
            ..Entity::default()
        };
        let fence = Entity {
            geometry: Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(0.0, 30.0),
                    Point2::new(10.0, 35.0),
                    Point2::new(20.0, 30.0),
                ],
                false,
            )),
            ..Entity::default()
        };
        let model = model_with(vec![parcel, tree, fence]);

        let written = must(export(&model, &path));
        assert_eq!(written.features_written, 3);

        let entities = dxf_entities(&path);
        assert_eq!(entities.len(), 3);
        let mut closed = 0;
        let mut open = 0;
        for entity in &entities {
            assert_ne!(
                entity.r#type, "HATCH",
                "a closed shape was written as a filled solid"
            );
            assert_eq!(entity.r#type, "LWPOLYLINE");
            if (entity.flags & 1) != 0 {
                closed += 1;
            } else {
                open += 1;
            }
        }
        assert_eq!(closed, 2, "the parcel and the tree are closed");
        assert_eq!(open, 1, "the fence is not");

        // And back: the parcel is a closed square again, its four corners
        // and no repeat of the first. By hand: the ring written is (0,0)
        // (20,0) (20,20) (0,20) (0,0), and the repeat is what the closed flag
        // stands for.
        let read = must(import(&path));
        let mut squares = 0;
        for entity in &read.entities {
            if let Geometry::Polyline(polyline) = &entity.geometry
                && polyline.vertices.len() == 4
            {
                squares += 1;
                assert!(polyline.closed);
                assert_eq!(polyline.area().abs(), 400.0);
            }
        }
        assert_eq!(squares, 1);
    }

    #[test]
    fn a_geo_package_of_thousands_of_entities_holds_every_one() {
        // Written in one transaction now rather than one per feature; the
        // count read back is what shows the transaction was committed, not
        // rolled back or left open.
        let dir = temp_dir("gpkg-many");
        let path = dir.file("points.gpkg");
        let entities: Vec<Entity> = (0..5000)
            .map(|i| Entity {
                geometry: point_at(f64::from(i), 2.0 * f64::from(i)),
                ..Entity::default()
            })
            .collect();
        let model = model_with(entities);

        let written = must(export(&model, &path));
        assert_eq!(written.features_written, 5000);

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 5000);
        let Some(Geometry::Point(last)) = read.entities.last().map(|entity| &entity.geometry)
        else {
            panic!("the last entity is a point");
        };
        assert_eq!(last.position, Point2::new(4999.0, 9998.0));
    }

    #[test]
    fn text_and_dimensions_are_skipped_and_counted() {
        let dir = temp_dir("skipped");
        let path = dir.file("mixed.geojson");

        let text = Entity {
            geometry: Geometry::Text(TextGeometry {
                position: Point2::new(1.0, 1.0),
                text: String::from("PARCEL 42"),
                height: 2.5,
                ..TextGeometry::default()
            }),
            ..Entity::default()
        };

        let model = model_with(vec![closed_square(10.0), text]);
        let result = must(export(&model, &path));

        // Skipped, never silently dropped: the count and a warning both say
        // so.
        assert_eq!(result.features_written, 1);
        assert_eq!(result.entities_skipped, 1);
        assert!(
            warned(&result.warnings, "text"),
            "the user must be told what was left out"
        );
    }

    #[test]
    fn mixed_geometry_into_a_shapefile_is_refused_without_writing_anything() {
        let dir = temp_dir("mixed-shp");
        let path = dir.file("mixed.shp");

        let line = Entity {
            geometry: segment(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
            ..Entity::default()
        };
        let model = model_with(vec![closed_square(10.0), line]);

        let error = refused(
            export(&model, &path),
            "a shapefile holds one geometry type per file",
        );
        assert_eq!(error.code, ErrorCode::Unsupported);

        // The refusal happens BEFORE anything is created. A half-written
        // shapefile is three or more files and is worse than no file,
        // because it opens.
        assert!(!path.exists());
        assert!(!dir.file("mixed.shx").exists());
        assert!(!dir.file("mixed.dbf").exists());
    }

    #[test]
    fn uniform_geometry_into_a_shapefile_succeeds() {
        let dir = temp_dir("uniform-shp");
        let path = dir.file("squares.shp");
        let model = model_with(vec![closed_square(10.0), closed_square(20.0)]);

        let result = must(export(&model, &path));
        assert_eq!(result.features_written, 2);
        assert!(path.exists());
    }

    #[test]
    fn curve_tessellation_honours_the_stated_tolerance() {
        let dir = temp_dir("curves");
        let path = dir.file("circle.geojson");

        const RADIUS: f64 = 5.0;
        const TOLERANCE: f64 = 0.001;
        let circle = Entity {
            geometry: Geometry::Circle(Circle2::new(Point2::new(0.0, 0.0), RADIUS)),
            ..Entity::default()
        };
        let model = model_with(vec![circle]);

        let options = VectorExportOptions {
            curve_tolerance: TOLERANCE,
            ..VectorExportOptions::default()
        };
        must(export_vector(&model, &path, &options));

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 1);
        let Geometry::Polyline(polygon) = &read.entities[0].geometry else {
            panic!("the circle comes back a polyline");
        };

        let sides = polygon.vertices.len() as f64;
        assert!(sides >= 3.0);

        // Every vertex lies exactly on the circle...
        for vertex in &polygon.vertices {
            near(vertex.x.hypot(vertex.y), RADIUS, 1e-9);
        }
        // ...and the deviation at the middle of a chord - the sagitta,
        // r(1 - cos(pi/n)) - is within the tolerance that was asked for. This
        // is the actual contract of curve_tolerance, and asserting the vertex
        // COUNT instead would only restate the implementation's formula back
        // at itself.
        let sagitta = RADIUS * (1.0 - (PI / sides).cos());
        assert!(sagitta <= TOLERANCE, "{sagitta}");
        // Not wastefully fine either: one side fewer would exceed the
        // tolerance, so the count is the smallest that satisfies it.
        let coarser = RADIUS * (1.0 - (PI / (sides - 1.0)).cos());
        assert!(coarser > TOLERANCE, "{coarser}");

        // The inscribed polygon's area follows n/2 r^2 sin(2 pi / n) exactly.
        let expected_area = 0.5 * sides * RADIUS * RADIUS * (TWO_PI / sides).sin();
        near(polygon.area().abs(), expected_area, 1e-9);
    }

    #[test]
    fn an_empty_selection_is_an_error_rather_than_an_empty_file() {
        let dir = temp_dir("empty");
        // No entities at all.
        let model = Model::default();
        let error = refused(
            export(&model, &dir.file("nothing.geojson")),
            "an empty drawing",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(!dir.file("nothing.geojson").exists());
    }
}

// ---- import -------------------------------------------------------------------------------------

mod interop_import {
    use super::*;

    #[test]
    fn a_dxf_circle_is_imported_as_a_closed_polyline_on_the_circle() {
        // GDAL reads a CIRCLE as a line string that repeats its first point;
        // taken as it came, that was an OPEN polyline with a seam where it
        // began.
        let dir = temp_dir("dxf-circle");
        let path = dir.file("tree.dxf");
        write_file(
            &path,
            "  0\nSECTION\n  2\nENTITIES\n  0\nCIRCLE\n  8\nTREES\n 10\n5.0\n 20\n7.0\n 30\n0.0\n \
             40\n0.6\n  0\nENDSEC\n  0\nEOF\n",
        );

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 1);
        let Geometry::Polyline(polyline) = &read.entities[0].geometry else {
            panic!("the circle comes back a polyline");
        };
        assert!(polyline.closed);
        assert!(polyline.vertices.len() >= 3);
        assert_ne!(
            polyline.vertices.first(),
            polyline.vertices.last(),
            "the closed flag stands for the repeat, which is not kept as well"
        );
        for vertex in &polyline.vertices {
            near((vertex.x - 5.0).hypot(vertex.y - 7.0), 0.6, 1e-9);
        }
    }

    #[test]
    fn only_a_line_that_ends_where_it_began_is_closed() {
        // The other side of the rule above: a line that ends elsewhere stays
        // open, and a line there and back (three points, ends equal) is not a
        // ring at all.
        let dir = temp_dir("geojson-open");
        let path = dir.file("lines.geojson");
        write_file(
            &path,
            r#"{"type":"FeatureCollection","features":[
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[0,0],[10,0],[10,5]]}},
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[0,0],[10,0],[0,0]]}},
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[0,0],[10,0],[10,5],[0,0]]}}
]}"#,
        );
        let read = must(import(&path));
        assert_eq!(read.entities.len(), 3);
        let polyline = |index: usize| match &read.entities[index].geometry {
            Geometry::Polyline(polyline) => polyline.clone(),
            other => panic!("entity {index} is {other:?}"),
        };
        let open = polyline(0);
        let there_and_back = polyline(1);
        let triangle = polyline(2);
        assert!(!open.closed);
        assert!(!there_and_back.closed);
        assert_eq!(there_and_back.vertices.len(), 3);
        assert!(triangle.closed);
        assert_eq!(triangle.vertices.len(), 3);
        // By hand: half of base 10 times height 5.
        assert_eq!(triangle.area().abs(), 25.0);
    }

    #[test]
    fn multi_geometry_is_flattened_and_attributes_are_copied_to_each_part() {
        let dir = temp_dir("multi");
        let path = dir.file("multi.geojson");

        // Written by hand rather than by our own exporter: a round trip
        // through our writer could not produce a MultiPolygon, so it would
        // never exercise the flattening path that real GIS data hits
        // constantly.
        write_file(
            &path,
            r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"parcel":"A1"},"geometry":{"type":"MultiPolygon",
       "coordinates":[[[[0,0],[10,0],[10,10],[0,10],[0,0]]],
                      [[[20,0],[30,0],[30,10],[20,10],[20,0]]]]}}]}"#,
        );

        let read = must(import(&path));
        assert_eq!(
            read.entities.len(),
            2,
            "one entity per polygon of the multi-geometry"
        );

        for entity in &read.entities {
            let Geometry::Polyline(polygon) = &entity.geometry else {
                panic!("each part is a polyline");
            };
            assert!(polygon.closed);
            // The closing vertex is dropped: `closed` expresses it instead,
            // and keeping it would make a zero-length final segment.
            assert_eq!(polygon.vertices.len(), 4);
            near(polygon.area().abs(), 100.0, 1e-9);

            assert_eq!(
                entity.properties.get("parcel"),
                Some(&PropertyValue::Text(String::from("A1"))),
                "each part keeps the feature's attributes"
            );
        }
    }

    #[test]
    fn two_point_lines_become_lines_and_longer_ones_polylines() {
        let dir = temp_dir("kinds");
        let path = dir.file("lines.geojson");
        write_file(
            &path,
            r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{},"geometry":{"type":"LineString",
       "coordinates":[[0,0],[10,0]]}},
      {"type":"Feature","properties":{},"geometry":{"type":"LineString",
       "coordinates":[[0,0],[10,0],[10,10]]}}]}"#,
        );

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 2);
        // A two-point line is a Line, not a two-vertex polyline: a drafter
        // expects to be able to fillet it.
        assert!(matches!(read.entities[0].geometry, Geometry::Line(_)));
        assert!(matches!(read.entities[1].geometry, Geometry::Polyline(_)));
    }

    #[test]
    fn layer_name_comes_from_the_source_and_is_reported() {
        let dir = temp_dir("layers");
        let path = dir.file("parcels.geojson");
        write_file(
            &path,
            r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[1,2]}}]}"#,
        );

        let read = must(import(&path));
        assert_eq!(read.layers_needed.len(), 1);
        // The caller must create these before executing the create command,
        // so they have to be reported rather than assumed to exist.
        assert!(!read.layers_needed[0].is_empty());
        assert_eq!(read.entities[0].layer, read.layers_needed[0]);

        // Provenance is recorded so an imported entity can be traced to its
        // file.
        assert_eq!(
            read.entities[0].metadata.get("source.file"),
            Some(&PropertyValue::Text(String::from("parcels.geojson")))
        );
    }

    #[test]
    fn an_origin_shift_is_applied_and_restored_on_export() {
        let dir = temp_dir("shift");
        let path = dir.file("far.geojson");
        // Survey coordinates, where a double has little resolution left.
        write_file(
            &path,
            r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{},"geometry":{"type":"Point",
       "coordinates":[500000.25,5000000.75]}}]}"#,
        );

        let options = VectorImportOptions {
            origin_shift: Some(Vec2::new(500000.0, 5000000.0)),
            ..VectorImportOptions::default()
        };
        let read = must(import_vector(&path, &options));

        let Geometry::Point(point) = &read.entities[0].geometry else {
            panic!("the point comes back a point");
        };
        double_eq(point.position.x, 0.25);
        double_eq(point.position.y, 0.75);

        // Exporting with the same shift puts it back exactly where it came
        // from.
        let model = model_with(vec![read.entities[0].clone()]);
        let export_options = VectorExportOptions {
            origin_shift: options.origin_shift,
            ..VectorExportOptions::default()
        };
        let back = dir.file("back.geojson");
        must(export_vector(&model, &back, &export_options));

        let reread = must(import(&back));
        let Geometry::Point(restored) = &reread.entities[0].geometry else {
            panic!("the point comes back a point");
        };
        double_eq(restored.position.x, 500000.25);
        double_eq(restored.position.y, 5000000.75);
    }

    #[test]
    fn imported_entities_pass_command_validation() {
        // The GUI and the CLI both feed import_vector's output straight into
        // create_entities, so whatever the importer produces has to survive
        // the command layer's validation - including the metadata it
        // attaches.
        let dir = temp_dir("validation");
        let path = dir.file("shapes.geojson");
        write_file(
            &path,
            r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"n":"1"},"geometry":{"type":"Polygon",
       "coordinates":[[[0,0],[10,0],[10,10],[0,10],[0,0]]]}}]}"#,
        );

        let read = must(import(&path));

        let mut model = Model::default();
        let mut stack = CommandStack::new(&mut model, None);
        let mut transaction = Transaction::new("IMPORT");
        for layer in &read.layers_needed {
            transaction.add(create_layer(Layer {
                name: layer.clone(),
                ..Layer::default()
            }));
        }
        transaction.add(create_entities(read.entities));

        must(stack.execute(&mut model, Box::new(transaction)));
        assert_eq!(model.entities.size(), 1);

        // And the whole import is a single undo step.
        must(stack.undo(&mut model));
        assert!(model.entities.empty());
    }
}

// ---- placement advice ---------------------------------------------------------------------------

fn box2(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

mod interop_placement {
    use super::*;

    #[test]
    fn data_far_from_the_drawing_is_reported_rather_than_merged_silently() {
        // The real case that prompted this: a DXF in a projected CRS at
        // (255440, 7410850) imported into a drawing that sits near the
        // origin. The merge succeeds, and at a zoom showing both the original
        // drawing is a dot smaller than a pixel - with nothing saying so.
        let drawing = box2(0.0, 0.0, 160.0, 100.0);
        let survey = box2(255440.07, 7410850.76, 257712.57, 7412122.21);

        let advice = advise_placement(&drawing, &survey);
        assert!(advice.far_apart);
        assert!(advice.separation > 7.0e6);
        assert!(!advice.message.is_empty());

        // The suggested shift brings the incoming data's corner onto the
        // drawing's.
        near(advice.suggested_shift.x, 255440.07, 1e-6);
        near(advice.suggested_shift.y, 7410850.76, 1e-6);
    }

    #[test]
    fn data_alongside_the_drawing_is_not_reported() {
        let drawing = box2(0.0, 0.0, 160.0, 100.0);

        // Overlapping.
        assert!(!advise_placement(&drawing, &box2(50.0, 20.0, 200.0, 150.0)).far_apart);
        // Adjacent, and of a comparable size: a neighbouring sheet, not a
        // different coordinate system.
        assert!(!advise_placement(&drawing, &box2(200.0, 0.0, 400.0, 100.0)).far_apart);
        // Ten times the size but in the same place - a site within a suburb.
        // Still not "far": the criterion is whether one becomes invisible,
        // and at 10x both are still legible.
        assert!(!advise_placement(&drawing, &box2(-500.0, -500.0, 1000.0, 1000.0)).far_apart);
    }

    #[test]
    fn importing_into_an_empty_drawing_is_never_far_apart() {
        // Nothing to be far FROM. Warning here would fire on every first
        // import, which is the normal way to start a job from survey data.
        let survey = box2(255440.07, 7410850.76, 257712.57, 7412122.21);
        assert!(!advise_placement(&Box2::default(), &survey).far_apart);
        assert!(!advise_placement(&survey, &Box2::default()).far_apart);
        assert!(
            advise_placement(&Box2::default(), &Box2::default())
                .message
                .is_empty()
        );
    }

    #[test]
    fn the_criterion_is_visibility_not_a_distance_in_metres() {
        // A fixed distance threshold would be wrong for a site plan and wrong
        // again for a national grid. THE SAME 20 km gap is far apart for two
        // 100 m drawings and not for two 50 km ones, and the arithmetic says
        // why:
        //
        //   100 m boxes, 20 km apart: each diagonal 141 m, combined view
        //                             20 100 m, so each is 0.70% of the view
        //                             - a dot.
        //   50 km boxes, 20 km apart: each diagonal 70 711 m, combined
        //                             130 000 m, so each is 54% of the view -
        //                             plainly visible.
        //
        // The threshold is 1% of the combined diagonal. (An earlier version of
        // this test used a 5 km gap for the small pair, which is 2.8% of the
        // view - small, but not invisible, so the implementation was right to
        // say no.)
        let gap = 20000.0;

        let small_a = box2(0.0, 0.0, 100.0, 100.0);
        let small_b = box2(gap, 0.0, gap + 100.0, 100.0);
        assert!(advise_placement(&small_a, &small_b).far_apart);

        let large_a = box2(0.0, 0.0, 50000.0, 50000.0);
        let large_b = box2(gap + 50000.0, 0.0, gap + 100000.0, 50000.0);
        assert!(!advise_placement(&large_a, &large_b).far_apart);
    }

    #[test]
    fn a_drawing_small_but_still_visible_is_not_reported() {
        // The boundary from the other side: two 100 m drawings 5 km apart
        // occupy 2.8% of a view showing both. Small, legible, and not worth
        // interrupting the user over - a warning that fires on ordinary
        // adjacent sheets would be ignored and would then be ignored when it
        // mattered.
        let a = box2(0.0, 0.0, 100.0, 100.0);
        let b = box2(5000.0, 0.0, 5100.0, 100.0);
        assert!(!advise_placement(&a, &b).far_apart);
    }

    #[test]
    fn small_data_placed_in_or_beside_the_drawing_is_not_far_apart() {
        // Audit IO-02. Correctly placed data that is merely small used to be
        // "far apart" with a separation of 0, and the default button moved
        // it to the drawing's corner. Each case worked by hand against a 2 km
        // site, diagonal 2 828.4 m:
        let site = box2(0.0, 0.0, 2000.0, 2000.0);

        // One control point in the middle: no extent, so never lost by its
        // size; the site is the whole combined view, 100% of it.
        let one_point = box2(500.0, 500.0, 500.0, 500.0);
        let point = advise_placement(&site, &one_point);
        assert!(!point.far_apart);
        assert!(point.message.is_empty());
        assert_eq!(point.suggested_shift.x, 0.0);
        assert_eq!(point.suggested_shift.y, 0.0);

        // A 10 m detail inside: 14.1 m, 0.5% of the view - but also 0.5% of
        // the site alone, so it would be as small wherever it sat. Either way
        // round.
        let detail = box2(1000.0, 1000.0, 1010.0, 1010.0);
        assert!(!advise_placement(&site, &detail).far_apart);
        assert!(!advise_placement(&detail, &site).far_apart);

        // The corner case the margin is for: a 20.5 m shed 79.5 m off the
        // site's corner. Combined view (0,0)-(2100,2100), diagonal 2 969.8 m;
        // the shed's diagonal 29.0 m is 0.98% of it, under the line, and
        // 1.025% of the site's, over it - lost "by placement" to a margin of
        // 1x, though showing both grows the view by only 5%. Against the 2%
        // margin (56.6 m) it is not.
        let shed = box2(2079.5, 2079.5, 2100.0, 2100.0);
        assert!(!advise_placement(&site, &shed).far_apart);
    }

    #[test]
    fn one_point_far_away_still_reports_the_drawing_it_would_hide() {
        // The case the advice exists for must survive IO-02's fix even when
        // the import is a single point: a control point in MGA coordinates
        // imported into a drawing near the origin. The point has no size to
        // lose, but the DRAWING, diagonal 188.7 m, is 0.0025% of a 7.4e6 m
        // view. The shift is the same corner-to-corner one as for any far
        // import.
        let drawing = box2(0.0, 0.0, 160.0, 100.0);
        let point = box2(255440.07, 7410850.76, 255440.07, 7410850.76);
        let advice = advise_placement(&drawing, &point);
        assert!(advice.far_apart);
        assert!(!advice.message.is_empty());
        near(advice.suggested_shift.x, 255440.07, 1e-6);
        near(advice.suggested_shift.y, 7410850.76, 1e-6);
    }

    #[test]
    fn two_lone_points_are_never_lost_however_far_apart() {
        // The degenerate case: neither part has an extent, so both draw as
        // markers at any zoom and neither can become invisible. Nothing to
        // warn about.
        let here = box2(0.0, 0.0, 0.0, 0.0);
        let there = box2(255440.07, 7410850.76, 255440.07, 7410850.76);
        assert!(!advise_placement(&here, &there).far_apart);
    }
}

// ---- heights (audit IO-01) ------------------------------------------------------------------------
//
// A vector file's Z is a height, and Katana keeps heights in the `elevation`
// / `elevations` properties every other part of it reads (katana_entity).
// Import used to drop Z and export to write Z = 0 - so a 3D DXF became a
// drawing on the datum, and a surveyed point went out at sea level.

fn heighted_point(x: f64, y: f64, z: Option<f64>) -> Entity {
    let mut entity = Entity {
        geometry: point_at(x, y),
        ..Entity::default()
    };
    set_heights(&mut entity.properties, &[z]);
    entity
}

/// The imported entity that came from model entity `id`, found by the
/// katana_id attribute the export writes. An Integer64 field, read back as
/// an integer since the import keeps field types (docs/interop.md,
/// "Fidelity").
fn by_katana_id(read: &VectorImportResult, id: EntityId) -> Option<&Entity> {
    read.entities.iter().find(|entity| {
        entity.properties.get("katana_id") == Some(&PropertyValue::Integer(id as i64))
    })
}

fn has_height_property(entity: &Entity) -> bool {
    entity.properties.contains_key(ELEVATION_PROPERTY)
        || entity.properties.contains_key(ELEVATIONS_PROPERTY)
}

fn polyline_of(vertices: &[(f64, f64)], closed: bool) -> Geometry {
    Geometry::Polyline(Polyline2::new(
        vertices.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
        closed,
    ))
}

mod interop_heights {
    use super::*;

    #[test]
    fn heights_go_into_the_geometry_and_come_back_exactly() {
        // Heights exact in binary except 0.1, which only has to round-trip.
        let dir = temp_dir("heights-geojson");
        let path = dir.file("heights.geojson");

        let mut string = Entity {
            geometry: polyline_of(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false),
            ..Entity::default()
        };
        set_heights(
            &mut string.properties,
            &[Some(10.25), Some(11.5), Some(0.1)],
        );
        let mut pad = closed_square(20.0);
        set_heights(&mut pad.properties, &[Some(7.0); 4]);
        let model = model_with(vec![
            heighted_point(5.0, 7.0, Some(32.5)),
            string,
            pad,
            heighted_point(1.0, 1.0, None),
        ]);

        must(export(&model, &path));
        assert!(
            !file_text(&path).contains("\"elevation"),
            "a height written into the geometry is not repeated as an attribute"
        );

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 4);

        let point = by_katana_id(&read, 1).expect("entity 1 comes back");
        assert_eq!(heights_of(&point.properties, 1)[0], Some(32.5));

        let back = by_katana_id(&read, 2).expect("entity 2 comes back");
        let heights = heights_of(&back.properties, 3);
        assert_eq!(heights[0], Some(10.25));
        assert_eq!(heights[1], Some(11.5));
        assert_eq!(heights[2], Some(0.1));

        let square = by_katana_id(&read, 3).expect("entity 3 comes back");
        for z in heights_of(&square.properties, 4) {
            assert_eq!(z, Some(7.0));
        }

        let flat = by_katana_id(&read, 4).expect("entity 4 comes back");
        assert!(
            !has_height_property(flat),
            "no height went out and none came back"
        );
    }

    #[test]
    fn a_real_zero_is_a_height_and_a_missing_z_is_none() {
        // The distinction the old import could not make: [5, 7, 0.0] is
        // surveyed at the datum, [1, 1] has no height at all.
        let dir = temp_dir("heights-zero");
        let path = dir.file("mixed.geojson");
        write_file(
            &path,
            r#"{"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"id": "datum"},
         "geometry": {"type": "Point", "coordinates": [5.0, 7.0, 0.0]}},
        {"type": "Feature", "properties": {"id": "plan"},
         "geometry": {"type": "Point", "coordinates": [1.0, 1.0]}}]}"#,
        );

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 2);
        for entity in &read.entities {
            let id = &entity.properties["id"];
            if *id == PropertyValue::Text(String::from("datum")) {
                assert_eq!(heights_of(&entity.properties, 1)[0], Some(0.0));
            } else {
                assert!(
                    !has_height_property(entity),
                    "a 2D point must not arrive at 0"
                );
            }
        }
    }

    #[test]
    fn a_vertical_step_loses_a_height_and_says_so() {
        // (0,0) at 1 then 5 is one vertex in plan: the model has no
        // zero-length segment, so the second is dropped - and with it a real
        // height.
        let dir = temp_dir("heights-step");
        let path = dir.file("step.geojson");
        write_file(
            &path,
            r#"{"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {},
         "geometry": {"type": "LineString",
                      "coordinates": [[0, 0, 1], [0, 0, 5], [10, 0, 6]]}}]}"#,
        );

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 1);
        let heights = heights_of(&read.entities[0].properties, 2);
        assert_eq!(heights[0], Some(1.0), "the first of the two is kept");
        assert_eq!(heights[1], Some(6.0));
        assert!(
            warned(&read.warnings, "1 vertices were dropped"),
            "the lost height is reported"
        );
    }

    #[test]
    fn a_string_that_comes_back_over_its_start_at_another_height_stays_open_with_every_height() {
        // A ramp that climbs round a 20 m square and ends one level above
        // where it began: its ends meet in plan only. Closing it would drop
        // the last vertex, and height 14 with it, for a ring the string never
        // was.
        let dir = temp_dir("heights-ramp");
        let path = dir.file("ramp.geojson");
        write_file(
            &path,
            r#"{"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"id": "ramp"},
         "geometry": {"type": "LineString",
                      "coordinates": [[0, 0, 10], [20, 0, 11], [20, 20, 12],
                                      [0, 20, 13], [0, 0, 14]]}},
        {"type": "Feature", "properties": {"id": "pad"},
         "geometry": {"type": "LineString",
                      "coordinates": [[0, 0, 7], [20, 0, 7], [20, 20, 7],
                                      [0, 20, 7], [0, 0, 7]]}}]}"#,
        );

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 2);
        for entity in &read.entities {
            let Geometry::Polyline(polyline) = &entity.geometry else {
                panic!("each string is a polyline");
            };
            if entity.properties["id"] == PropertyValue::Text(String::from("ramp")) {
                assert!(!polyline.closed, "its ends are 4 m apart in height");
                assert_eq!(polyline.vertices.len(), 5);
                assert_eq!(polyline.vertices[4], Point2::new(0.0, 0.0));
                // By hand: the file's five heights, one per vertex, in order.
                let heights = heights_of(&entity.properties, 5);
                for (i, height) in heights.iter().enumerate() {
                    assert_eq!(*height, Some(10.0 + i as f64));
                }
            } else {
                // The same square with its ends at one height IS a ring - how
                // a closed 3D polyline comes out of a DXF - so it still
                // closes, with its four corners.
                assert!(polyline.closed);
                assert_eq!(polyline.vertices.len(), 4);
                for z in heights_of(&entity.properties, 4) {
                    assert_eq!(z, Some(7.0));
                }
            }
        }
        assert!(
            !warned(&read.warnings, "vertices were dropped"),
            "no height was lost"
        );
    }

    #[test]
    fn an_open3_d_string_that_returns_over_its_start_in_plan_comes_back_open() {
        // Katana's own open string, round-tripped: the export writes its five
        // vertices with their heights, and the import must not read the equal
        // ends in plan as a ring.
        let dir = temp_dir("heights-ramp-trip");
        let path = dir.file("ramp.geojson");
        let vertices = [
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 20.0),
            (0.0, 20.0),
            (0.0, 0.0),
        ];
        let mut ramp = Entity {
            geometry: polyline_of(&vertices, false),
            ..Entity::default()
        };
        set_heights(
            &mut ramp.properties,
            &[Some(10.0), Some(11.0), Some(12.0), Some(13.0), Some(14.0)],
        );
        let model = model_with(vec![ramp]);

        must(export(&model, &path));
        let read = must(import(&path));
        assert_eq!(read.entities.len(), 1);
        let Geometry::Polyline(back) = &read.entities[0].geometry else {
            panic!("the string comes back a polyline");
        };
        assert!(!back.closed);
        let expected: Vec<Point2> = vertices.iter().map(|&(x, y)| Point2::new(x, y)).collect();
        assert_eq!(back.vertices, expected);
        let heights = heights_of(&read.entities[0].properties, 5);
        assert_eq!(heights[4], Some(14.0), "the top of the ramp is kept");
    }

    #[test]
    fn a_string_heighted_at_only_some_vertices_goes_in_plan_with_its_heights_kept() {
        // A 3D geometry needs a height at every vertex; writing 0 for the
        // missing one would be a height nobody measured. So the string goes
        // in plan and the list travels as an attribute, and reading it back
        // restores it.
        let dir = temp_dir("heights-partial");
        let path = dir.file("partial.geojson");

        let mut string = Entity {
            geometry: polyline_of(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false),
            ..Entity::default()
        };
        set_heights(&mut string.properties, &[Some(1.0), None, Some(3.0)]);
        let model = model_with(vec![string]);

        let written = must(export(&model, &path));
        assert!(warned(&written.warnings, "only some of their vertices"));

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 1);
        let heights = heights_of(&read.entities[0].properties, 3);
        assert_eq!(heights[0], Some(1.0));
        assert!(heights[1].is_none());
        assert_eq!(heights[2], Some(3.0));
    }

    #[test]
    fn a_shapefile_of_heighted_points_is_written3_d() {
        let dir = temp_dir("heights-shp");
        let path = dir.file("points.shp");
        let model = model_with(vec![
            heighted_point(0.0, 0.0, Some(12.5)),
            heighted_point(5.0, 5.0, Some(13.75)),
        ]);

        let written = must(export(&model, &path));
        assert!(!warned(&written.warnings, "written in plan"));

        let read = must(import(&path));
        let first = by_katana_id(&read, 1).expect("entity 1 comes back");
        let second = by_katana_id(&read, 2).expect("entity 2 comes back");
        assert_eq!(heights_of(&first.properties, 1)[0], Some(12.5));
        assert_eq!(heights_of(&second.properties, 1)[0], Some(13.75));
    }

    #[test]
    fn a_shapefile_mixing_heighted_and_heightless_entities_is_written_in_plan_and_says_so() {
        // A shapefile layer is all 2D or all 3D, and a 3D one has no "no
        // height": the heightless point would be written at 0. So the layer
        // goes in plan, the heights ride as attributes, and the round trip
        // still keeps them.
        let dir = temp_dir("heights-shp-mixed");
        let path = dir.file("mixed.shp");
        let model = model_with(vec![
            heighted_point(0.0, 0.0, Some(32.5)),
            heighted_point(5.0, 5.0, None),
        ]);

        let written = must(export(&model, &path));
        assert!(warned(&written.warnings, "1 of 2 entities have heights"));

        let read = must(import(&path));
        let heighted = by_katana_id(&read, 1).expect("entity 1 comes back");
        let heightless = by_katana_id(&read, 2).expect("entity 2 comes back");
        assert_eq!(heights_of(&heighted.properties, 1)[0], Some(32.5));
        assert!(
            heights_of(&heightless.properties, 1)[0].is_none(),
            "the heightless point must not come back at 0"
        );
    }

    #[test]
    fn a_dxf_point_keeps_its_height() {
        // The audit's case: a DXF carries heights only in its geometry,
        // because its fields are fixed and an "elevation" attribute has
        // nowhere to go.
        let dir = temp_dir("heights-dxf");
        let path = dir.file("survey.dxf");
        let mut string = Entity {
            geometry: segment(
                Point2::new(255440.0, 7410850.0),
                Point2::new(255450.0, 7410860.0),
            ),
            ..Entity::default()
        };
        set_heights(&mut string.properties, &[Some(32.5), Some(33.25)]);
        let model = model_with(vec![
            heighted_point(255440.125, 7410850.25, Some(32.5)),
            string,
        ]);

        must(export(&model, &path));

        let read = must(import(&path));
        assert_eq!(read.entities.len(), 2);
        let mut saw_point = false;
        let mut saw_line = false;
        for entity in &read.entities {
            match &entity.geometry {
                Geometry::Point(_) => {
                    assert_eq!(heights_of(&entity.properties, 1)[0], Some(32.5));
                    saw_point = true;
                }
                Geometry::Line(_) => {
                    let heights = heights_of(&entity.properties, 2);
                    assert_eq!(heights[0], Some(32.5));
                    assert_eq!(heights[1], Some(33.25));
                    saw_line = true;
                }
                _ => {}
            }
        }
        assert!(saw_point);
        assert!(saw_line);
    }

    #[test]
    fn a_geo_package_holds_heighted_and_heightless_entities_together() {
        // Unlike a shapefile, each GeoPackage geometry has its own dimension,
        // so a mixed drawing keeps its heights in the geometry and nothing
        // goes in plan.
        let dir = temp_dir("heights-gpkg");
        let path = dir.file("mixed.gpkg");
        let model = model_with(vec![
            heighted_point(0.0, 0.0, Some(32.5)),
            heighted_point(5.0, 5.0, None),
        ]);

        let written = must(export(&model, &path));
        assert!(!warned(&written.warnings, "written in plan"));

        let read = must(import(&path));
        let heighted = by_katana_id(&read, 1).expect("entity 1 comes back");
        let heightless = by_katana_id(&read, 2).expect("entity 2 comes back");
        assert_eq!(heights_of(&heighted.properties, 1)[0], Some(32.5));
        assert!(
            !has_height_property(heightless),
            "the heightless point must not come back at 0"
        );
    }
}
