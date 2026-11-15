//! tests/interop/test_archive12d.cpp, its archive cases: interop's handling
//! of the .12da and .12daz files - reading their bytes, the ZIP container,
//! and the point clouds an archive carries. The file's ZipContainer cases are
//! katana_gis's (tests/gdal_core).

use std::path::Path;

use katana_core::ErrorCode;
use katana_entity::{Color, Entity, Geometry, Model, PointGeometry, PropertyValue};
use katana_geometry::{Point2, Polyline2, Vec2};
use katana_gis::zip_container::{list_zip, write_zip};
use katana_interop::archive12d::{
    Archive12dExportOptions, Archive12dImportOptions, export_archive12d, import_archive12d,
    is_zipped_archive12d,
};
use katana_interop::export::export_point_cloud;
use katana_interop::import::{SourceKind, kind_for_path};
use katana_interop::reference_data::PointCloudLayer;
use katana_pointcloud::PointCloudPoint;

use crate::{TempDir, archive_fixture, bytes_of, must, refused, warned, write_file};

fn temp_dir(name: &str) -> TempDir {
    TempDir::new("katana-archive12d-", name)
}

fn import(path: &Path) -> katana_core::Result<katana_interop::Archive12dImportResult> {
    import_archive12d(path, &Archive12dImportOptions::default())
}

fn export(
    model: &Model,
    path: &Path,
) -> katana_core::Result<katana_interop::Archive12dExportResult> {
    export_archive12d(model, &[], path, &Archive12dExportOptions::default(), &[])
}

fn small_drawing() -> Model {
    let mut model = Model::default();
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(502000.5, 6960000.25),
        }),
        ..Entity::default()
    };
    point
        .properties
        .insert(String::from("elevation"), PropertyValue::Real(32.5));
    must(model.entities.add(point));
    let line = Entity {
        geometry: Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 5.0),
            ],
            true,
        )),
        ..Entity::default()
    };
    must(model.entities.add(line));
    model
}

fn point_of(entity: &Entity) -> Point2 {
    match &entity.geometry {
        Geometry::Point(point) => point.position,
        other => panic!("not a point: {other:?}"),
    }
}

mod archive_import {
    use super::*;

    #[test]
    fn zipped_leaders_and_custom_colours_reach_the_domain_together() {
        let dir = temp_dir("drafting-colours");
        let file = dir.file("notes.12daz");
        must(write_zip(
            &file,
            "notes.12da",
            &bytes_of(&archive_fixture("drafting_leaders.12da")),
        ));
        let mut options = Archive12dImportOptions::default();
        must(
            options
                .colours
                .add("test utility amber", Color::new(224, 128, 32, 255)),
        );
        must(
            options
                .colours
                .add("test utility green", Color::new(32, 112, 80, 255)),
        );
        let imported = must(import_archive12d(&file, &options));
        assert_eq!(imported.entities.len(), 4);
        assert_eq!(
            imported.entities[0].color,
            Some(Color::new(224, 128, 32, 255))
        );
        assert_eq!(
            imported.entities[1].color,
            Some(Color::new(32, 112, 80, 255))
        );
        assert!(matches!(imported.entities[2].geometry, Geometry::Leader(_)));
        assert!(matches!(imported.entities[3].geometry, Geometry::Leader(_)));
        assert_eq!(imported.member_name, "notes.12da");
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    }
}

mod archive12d_kinds {
    use super::*;

    #[test]
    fn both_extensions_are_recognised_whatever_their_case_and_no_other() {
        for name in ["job.12da", "job.12daz", "JOB.12DA", "Job.12Daz"] {
            assert_eq!(
                kind_for_path(Path::new(name)),
                SourceKind::Archive12d,
                "{name}"
            );
        }
        assert!(!is_zipped_archive12d(Path::new("job.12da")));
        assert!(is_zipped_archive12d(Path::new("job.12DAZ")));
        // There is no such format; it was accepted for a while by mistake.
        assert_ne!(kind_for_path(Path::new("job.12dz")), SourceKind::Archive12d);
        assert!(!is_zipped_archive12d(Path::new("job.12dz")));
        assert_ne!(kind_for_path(Path::new("job.12d")), SourceKind::Archive12d);
    }
}

mod archive12d_import {
    use super::*;

    #[test]
    fn a_zipped_archive_gives_what_the_plain_file_gives() {
        let plain = must(import(&archive_fixture("all_geometries.12da")));
        let zipped = must(import(&archive_fixture("all_geometries.12daz")));
        // The member is NOT named after the zip: the importer has to find it.
        assert_eq!(zipped.member_name, "comprehensive-all-geometries.12da");
        assert!(plain.member_name.is_empty());

        assert_eq!(zipped.entities.len(), plain.entities.len());
        assert_eq!(zipped.alignments.len(), plain.alignments.len());
        assert_eq!(zipped.tally, plain.tally);
        assert_eq!(zipped.archive_version, "15.01.08.60");
        assert_eq!(zipped.encoding, "UTF-8");
        // What the fixture is known to hold (counted by hand in its own
        // header): one tin of 4 triangles and a cloud of 3 points - and the
        // super tin "Site Surfaces" built from that tin, which is a separate
        // object in the format and so a separate surface here.
        assert_eq!(zipped.surfaces.len(), 2);
        assert_eq!(zipped.surfaces[0].surface.triangle_count(), 4);
        assert_eq!(zipped.surfaces[1].name, "Site Surfaces");
        assert_eq!(
            zipped.surfaces[1].surface.triangle_count(),
            4,
            "a super tin of one member is that member"
        );
        assert_eq!(zipped.clouds.len(), 1);
        assert_eq!(zipped.clouds[0].points.len(), 3);
        assert_eq!(zipped.clouds[0].name, "LiDAR Patch");
        assert!(!zipped.clouds[0].bounds.empty());
        assert_eq!(zipped.clouds[0].bounds.min_x, 503700.0);
        assert_eq!(zipped.clouds[0].bounds.max_z, 30.7);
    }

    #[test]
    fn every_entity_is_one_the_model_will_accept() {
        // The point of validating inside the importer: create_entities is
        // atomic, so ONE bad entity would cost the user the whole file.
        let imported = must(import(&archive_fixture("all_geometries.12da")));
        let mut model = Model::default();
        for layer in &imported.layers_needed {
            assert!(model.layers.add(layer.clone()).is_ok(), "{}", layer.name);
        }
        for style in &imported.styles_needed {
            assert!(model.styles.add(style.clone()).is_ok(), "{}", style.name);
        }
        assert!(!imported.entities.is_empty());
        assert!(
            !imported.styles_needed.is_empty(),
            "the fixture names linestyles"
        );
        for entity in &imported.entities {
            assert!(model.layers.contains(&entity.layer), "{}", entity.layer);
            assert!(
                entity.style.is_empty() || model.styles.contains(&entity.style),
                "{}",
                entity.style
            );
            assert!(model.entities.add(entity.clone()).is_ok());
        }
        for alignment in &imported.alignments {
            assert!(
                model.alignments.add(alignment.clone()).is_ok(),
                "{}",
                alignment.name
            );
        }
    }

    #[test]
    fn failures_name_the_file_and_the_line() {
        let dir = temp_dir("import-bad");
        assert_eq!(
            refused(import(&dir.file("missing.12da")), "a missing archive").code,
            ErrorCode::FileImportFailure
        );

        write_file(
            &dir.file("broken.12da"),
            "model \"M\"\nstring super {\n data_3d { 1 2 }\n}\n",
        );
        let broken = refused(import(&dir.file("broken.12da")), "a broken archive");
        assert_eq!(broken.code, ErrorCode::ParseFailure);
        assert!(
            broken.context.contains("broken.12da"),
            "{}",
            broken.describe()
        );
        assert!(broken.context.contains("line 3"), "{}", broken.describe());

        write_file(&dir.file("plain.12daz"), "model \"M\"\n");
        assert!(import(&dir.file("plain.12daz")).is_err(), "not a zip");

        must(write_zip(
            &dir.file("empty.12daz"),
            "readme.txt",
            b"no archive here",
        ));
        let none = refused(import(&dir.file("empty.12daz")), "a zip without an archive");
        assert!(none.message.contains("no .12da"), "{}", none.message);

        let tiny = Archive12dImportOptions {
            max_bytes: 10,
            ..Archive12dImportOptions::default()
        };
        let too_big = refused(
            import_archive12d(&archive_fixture("all_geometries.12da"), &tiny),
            "an archive past the limit",
        );
        assert!(too_big.message.contains("limit"), "{}", too_big.message);
        assert!(import_archive12d(&archive_fixture("all_geometries.12daz"), &tiny).is_err());
    }

    #[test]
    fn an_empty_file_is_an_empty_import_not_an_error() {
        let dir = temp_dir("import-empty");
        write_file(&dir.file("empty.12da"), "");
        let imported = must(import(&dir.file("empty.12da")));
        assert!(imported.entities.is_empty());
        assert!(imported.bounds.empty());
    }

    // ---- referenced point clouds ------------------------------------------------------------

    #[test]
    fn a_ref_data_cloud_is_read_when_the_las_file_sits_beside_the_archive() {
        // The archive writes a ref_data cloud as a PATH into a project that
        // the archive has left behind. The file is looked for beside the
        // archive - where the two travel together when a job is sent on - and
        // read when it is there.
        let directory = temp_dir("refdata-found");
        let mut scan = PointCloudLayer {
            name: String::from("scan"),
            ..PointCloudLayer::default()
        };
        for i in 0..12 {
            scan.points.push(PointCloudPoint {
                x: 502000.0 + f64::from(i),
                y: 6960000.0,
                z: 30.0 + f64::from(i) * 0.5,
                ..PointCloudPoint::default()
            });
        }
        let las = directory.file("site.las");
        must(export_point_cloud(&scan, &las));

        // The archive names it by a path out of a directory that is not
        // there.
        let archive = directory.file("job.12da");
        write_file(
            &archive,
            r#"string las_cloud_data { name "Survey Scan"
  ref_data { file_name "..\..\scans\site.las" } }"#,
        );

        let imported = must(import(&archive));
        assert_eq!(imported.clouds.len(), 1);
        assert_eq!(
            imported.clouds[0].name, "Survey Scan",
            "the name the archive gave it"
        );
        assert_eq!(imported.clouds[0].points.len(), 12);
        assert!(
            warned(&imported.warnings, "found beside the archive"),
            "and it says it went and got it"
        );
    }

    #[test]
    fn a_ref_data_cloud_that_is_not_there_is_reported_and_not_chased() {
        let directory = temp_dir("refdata-missing");
        let archive = directory.file("job.12da");
        write_file(
            &archive,
            r#"string las_cloud_data { name "Missing" ref_data { file_name "nowhere.las" } }"#,
        );

        let imported = must(import(&archive));
        assert!(imported.clouds.is_empty());
        assert!(
            warned(&imported.warnings, "not beside the archive"),
            "a missing scan is a fact about the file, not a failure of the import"
        );
    }
}

mod archive12d_export {
    use super::*;

    #[test]
    fn a_plain_archive_is_utf16_with_a_mark_by_default_and_utf8_on_request() {
        let dir = temp_dir("export-encoding");
        let model = small_drawing();
        must(export(&model, &dir.file("wide.12da")));
        let wide = bytes_of(&dir.file("wide.12da"));
        assert!(wide.len() >= 4);
        assert_eq!(
            &wide[..2],
            b"\xFF\xFE",
            "the mark the archive's program writes"
        );
        assert_eq!(wide[2], b'/');
        assert_eq!(wide[3], 0);

        let options = Archive12dExportOptions {
            utf16: false,
            ..Archive12dExportOptions::default()
        };
        must(export_archive12d(
            &model,
            &[],
            &dir.file("narrow.12da"),
            &options,
            &[],
        ));
        assert_eq!(&bytes_of(&dir.file("narrow.12da"))[..2], b"//");

        let back = must(import(&dir.file("wide.12da")));
        assert_eq!(back.encoding, "UTF-16 little-endian");
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    }

    #[test]
    fn a_zipped_export_holds_one_member_named_after_the_file_and_imports_back() {
        let dir = temp_dir("export-zip");
        let model = small_drawing();
        let exported = must(export(&model, &dir.file("Site Survey.12daz")));
        assert_eq!(exported.entities_written, 2);

        let members = must(list_zip(&dir.file("Site Survey.12daz")));
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "Site Survey.12da");
        assert_eq!(members[0].size, exported.bytes_written);

        let back = must(import(&dir.file("Site Survey.12daz")));
        assert_eq!(back.entities.len(), 2);
        assert_eq!(
            point_of(&back.entities[0]),
            Point2::new(502000.5, 6960000.25)
        );
        assert_eq!(
            back.entities[0].properties["elevation"],
            PropertyValue::Real(32.5)
        );
        let Geometry::Polyline(ring) = &back.entities[1].geometry else {
            panic!("the ring comes back a polyline");
        };
        assert!(ring.closed);
        assert_eq!(ring.vertices.len(), 3);
        assert_eq!(
            back.entities[0].metadata["source"],
            PropertyValue::Text(String::from("Site Survey.12daz"))
        );
    }

    #[test]
    fn a_name_that_is_not_an_archives_is_refused_before_anything_is_written() {
        let dir = temp_dir("export-name");
        let model = small_drawing();
        let wrong = refused(
            export(&model, &dir.file("drawing.dxf")),
            "a name that is not an archive's",
        );
        assert_eq!(wrong.code, ErrorCode::InvalidArgument);
        assert!(!dir.file("drawing.dxf").exists());
    }

    #[test]
    fn an_origin_shift_taken_on_import_is_given_back_on_export() {
        let dir = temp_dir("export-shift");
        let input = Archive12dImportOptions {
            origin_shift: Some(Vec2::new(502000.0, 6960000.0)),
            ..Archive12dImportOptions::default()
        };
        let imported = must(import_archive12d(
            &archive_fixture("all_geometries.12da"),
            &input,
        ));
        // The first entity is the permanent mark at 502000, 6960000.
        assert_eq!(point_of(&imported.entities[0]), Point2::new(0.0, 0.0));
        assert!(imported.bounds.max.x < 2000.0);
        // The tin, and the super tin built from it: the fixture's "Site
        // Surfaces" names "Natural Surface TIN", and a tin and a super tin
        // are separate objects in the format, so both arrive.
        assert_eq!(imported.surfaces.len(), 2);
        assert_eq!(imported.surfaces[1].name, "Site Surfaces");
        for surface in &imported.surfaces {
            assert!(
                surface.surface.bounds().max.x < 2000.0,
                "surfaces move with it"
            );
        }
        assert_eq!(imported.clouds.len(), 1);
        assert_eq!(imported.clouds[0].bounds.min_x, 1700.0, "and so do clouds");

        let mut model = Model::default();
        for layer in &imported.layers_needed {
            must(model.layers.add(layer.clone()));
        }
        must(model.entities.add(imported.entities[0].clone()));
        let out = Archive12dExportOptions {
            origin_shift: input.origin_shift,
            ..Archive12dExportOptions::default()
        };
        must(export_archive12d(
            &model,
            &[],
            &dir.file("back.12da"),
            &out,
            &[],
        ));
        let back = must(import(&dir.file("back.12da")));
        assert_eq!(
            point_of(&back.entities[0]),
            Point2::new(502000.0, 6960000.0)
        );
    }
}
