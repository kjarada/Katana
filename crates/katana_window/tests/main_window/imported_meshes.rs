//! An archive's mesh reaches the views in the colour its import resolved: MeshItem::imported, the
//! one mapping the window's import uses, gives the mesh's colour as the `flat_color` that BOTH the
//! plan's footprint and the 3D view draw with, and a face's own colour before it in 3D.
//!
//! The fixture is tests/archive12d/data/indexed_colours.12da (invented): a mesh coloured
//! "acad 001" and one coloured "acad 005" whose faces are "acad 010" and "vis something". The
//! RGB are the published AutoCAD Colour Index's: 1 red, 5 blue, 10 red.

use std::path::PathBuf;

use katana_archive12d::{ImportOptions, ReadOptions, read_archive_bytes, to_domain};
use katana_entity::Model;
use katana_render::rgba;
use katana_window::main_window::SceneStore;
use katana_window::main_window::executor::MeshItem;

fn meshes() -> Vec<katana_archive12d::ImportedMesh> {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/archive12d/data/indexed_colours.12da"
    ));
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let archive = read_archive_bytes(bytes, &ReadOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    to_domain(&archive, &ImportOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
        .meshes
}

mod imported_meshes {
    use super::*;

    #[test]
    fn a_mesh_coloured_by_an_index_name_is_drawn_in_that_colour_in_plan_and_in_3d() {
        let items: Vec<MeshItem> = meshes().into_iter().map(MeshItem::imported).collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].scene.flat_color, rgba(255, 0, 0), "acad 001");
        assert_eq!(items[1].scene.flat_color, rgba(0, 0, 255), "acad 005");
        // The first has no face colours; the second's are its faces' own, and the face whose name
        // nothing defines takes the mesh's colour rather than the default clay.
        assert!(items[0].scene.face_colors.is_empty());
        assert_eq!(
            items[1].scene.face_colors,
            [rgba(255, 0, 0), rgba(0, 0, 255)]
        );

        // And what the views are lent carries them: the plan's footprint painter and the 3D
        // scene both read SceneStore::meshes.
        let store = SceneStore {
            meshes: items,
            ..SceneStore::default()
        };
        let lent = store.meshes(&Model::default());
        assert_eq!(lent.len(), 2);
        assert_eq!(lent[0].flat_color, rgba(255, 0, 0));
        assert_eq!(lent[1].flat_color, rgba(0, 0, 255));
        assert!(lent.iter().all(|mesh| mesh.mesh.is_some()));
    }

    #[test]
    fn a_mesh_whose_colour_nothing_defines_is_drawn_in_the_default_clay() {
        let mut mesh = meshes().remove(0);
        mesh.color = None;
        mesh.face_colors.clear();
        let item = MeshItem::imported(mesh);
        assert_eq!(item.scene.flat_color, rgba(190, 170, 140));
    }
}
