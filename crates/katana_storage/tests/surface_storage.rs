//! The twin of tests/storage/test_surface_storage.cpp: the drawing's surfaces
//! in a project (schema 13; docs/terrain.md, "Surfaces in the drawing") -
//! opaque records kept in the same transaction as the rest, in their order,
//! every field as it was - and refused before a byte is written when a load
//! could not give them back.

pub mod support;

use std::path::{Path, PathBuf};

use katana_core::ErrorCode;
use katana_entity::{Entity, Geometry, Model};
use katana_geometry::{Point2, Segment2};
use katana_storage::{
    OpenMode, ProjectContents, ProjectMetadata, ProjectStore, SqliteDatabase, SurfaceRecord,
    capture_model,
};
use support::Scratch;

fn scratch(case: &str) -> (Scratch, PathBuf) {
    let scratch = Scratch::new("SurfaceStorage", case);
    let project = scratch.project_dir();
    (scratch, project)
}

fn create(project: &Path) -> ProjectStore {
    ProjectStore::create(project, &ProjectMetadata::default()).unwrap()
}

// Bytes a TEXT column would mangle - a NUL, a lone CR, bytes that are not
// UTF-8 - since the record's data is the terrain layer's and opaque here.
fn record(name: &str, style: &str, visible: bool) -> SurfaceRecord {
    let mut data = b"KTIN\0\r\xff\xfe opaque".to_vec();
    data.extend_from_slice(name.as_bytes());
    SurfaceRecord {
        name: name.as_bytes().to_vec(),
        source: b"point cloud site.laz".to_vec(),
        style: style.as_bytes().to_vec(),
        visible,
        data,
    }
}

fn contents_with(surfaces: Vec<SurfaceRecord>) -> ProjectContents {
    let mut model = Model::default();
    let line = Entity {
        geometry: Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0))),
        ..Entity::default()
    };
    assert!(model.entities.add(line).is_ok());
    let mut contents = capture_model(&model, ProjectMetadata::default(), Vec::new());
    contents.surfaces = surfaces;
    contents
}

mod surface_storage {
    use super::*;

    #[test]
    fn surfaces_survive_saving_twice_and_reopening_in_their_order_with_every_field() {
        let (_scratch, project) =
            scratch("SurfacesSurviveSavingTwiceAndReopeningInTheirOrderWithEveryField");
        let surfaces = vec![
            record("ground", "", true),
            record("design pad", "Contours", false),
            record("Alpha", "Cut Fill", true),
        ];
        {
            let mut store = create(&project);
            store.save(&contents_with(surfaces.clone())).unwrap();
            // Twice: a table missing from save's DELETE list fails the second
            // on its primary key, not the first.
            store.save(&contents_with(surfaces.clone())).unwrap();
        }
        let store = ProjectStore::open(&project).unwrap();
        assert_eq!(store.schema_version().unwrap(), 13);
        let loaded = store.load().unwrap();
        // In the order they were saved - "Alpha" last, not sorted first.
        assert_eq!(loaded.surfaces, surfaces);
        assert_eq!(
            loaded.entities.len(),
            1,
            "saved in the one transaction with the drawing"
        );
    }

    #[test]
    fn a_save_with_no_surfaces_leaves_none_of_the_last_saves_behind() {
        let (_scratch, project) = scratch("ASaveWithNoSurfacesLeavesNoneOfTheLastSavesBehind");
        let mut store = create(&project);
        store
            .save(&contents_with(vec![record("ground", "", true)]))
            .unwrap();
        store.save(&contents_with(Vec::new())).unwrap();
        let loaded = store.load().unwrap();
        assert!(loaded.surfaces.is_empty());
    }

    #[test]
    fn a_schema_twelve_project_migrates_to_thirteen_with_no_surfaces() {
        let (_scratch, project) = scratch("ASchemaTwelveProjectMigratesToThirteenWithNoSurfaces");
        {
            let mut store = create(&project);
            store.save(&contents_with(Vec::new())).unwrap();
        }
        {
            // Schema 12 as the build before this one left it: no surfaces
            // table.
            let raw = SqliteDatabase::open(&project.join("project.db"), OpenMode::ReadWriteCreate)
                .unwrap();
            raw.execute("DROP TABLE surfaces;").unwrap();
            raw.set_user_version(12).unwrap();
        }
        let mut store = ProjectStore::open(&project).unwrap();
        assert_eq!(store.schema_version().unwrap(), 13);
        // Migrated only with a way back, as every migration is.
        assert_eq!(store.list_backups().len(), 1);
        let mut loaded = store.load().unwrap();
        assert!(loaded.surfaces.is_empty());
        assert_eq!(loaded.entities.len(), 1, "the drawing opens as it was");
        // And it keeps a surface like any project of this schema.
        loaded.surfaces.push(record("ground", "", true));
        store.save(&loaded).unwrap();
        let again = store.load().unwrap();
        assert_eq!(again.surfaces.len(), 1);
        assert_eq!(again.surfaces[0], record("ground", "", true));
    }

    #[test]
    fn a_surface_a_load_could_not_give_back_is_refused_and_nothing_is_written() {
        let (_scratch, project) =
            scratch("ASurfaceALoadCouldNotGiveBackIsRefusedAndNothingIsWritten");
        let mut store = create(&project);
        let kept = vec![record("ground", "", true)];
        store.save(&contents_with(kept.clone())).unwrap();

        let mut unnamed = record("x", "", true);
        unnamed.name.clear();
        let mut empty = record("empty", "", true);
        empty.data.clear();
        // The drawing's store compares names without case, so these two could
        // not both be loaded into it.
        let refused = [
            vec![unnamed],
            vec![empty],
            vec![record("Ground", "", true), record("GROUND", "", true)],
        ];
        for surfaces in refused {
            let status = store
                .save(&contents_with(surfaces))
                .expect_err("a surface a load could not give back was saved");
            assert_eq!(
                status.code,
                ErrorCode::InvalidArgument,
                "{}",
                status.describe()
            );
        }
        let loaded = store.load().unwrap();
        assert_eq!(loaded.surfaces, kept, "the last good save stands");
    }

    #[test]
    #[allow(
        clippy::assertions_on_constants,
        reason = "The C++ case checks the constant at run time, beside the sizes it governs; kept as its twin."
    )]
    fn a_surface_too_large_for_the_database_is_refused_saying_what_to_do() {
        let _scratch = scratch("ASurfaceTooLargeForTheDatabaseIsRefusedSayingWhatToDo");
        let surface = record("ground", "Contours", true);
        // Its row: name 6 + source 20 ("point cloud site.laz") + style 8 +
        // data 15 + 6 + the 8 for the flag and the position = 63 bytes.
        assert_eq!(ProjectStore::surface_row_bytes(&surface), 63);
        assert!(ProjectStore::check_surface_size(&surface, 63).is_ok());
        let over = ProjectStore::check_surface_size(&surface, 62)
            .expect_err("a surface over its limit passed");
        assert_eq!(over.code, ErrorCode::InvalidArgument);
        assert!(over.message.contains("ground"));
        assert!(over.message.contains("max="), "says what to do instead");
        // The limit is SQLite's default SQLITE_MAX_LENGTH, 1e9, less headroom.
        assert!(ProjectStore::MAX_SURFACE_BYTES < 1_000_000_000);
    }
}
