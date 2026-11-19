//! The twin of tests/archive12d/test_meshes.cpp: an archive's trimesh becomes
//! a mesh in the session and goes back out as one (PLAN.MD 20.2, slice 4).

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_archive12d::domain::standard_colour;
use katana_archive12d::{
    ExportMesh, ExportOptions, ImportOptions, ReadOptions, Trimesh, WriteOptions, from_domain,
    read_archive, to_domain, write_archive,
};
use katana_entity::Model;
use katana_geometry::mesh::validate;
use katana_geometry::{Point3, TriangleMesh};

use support::{any_contains, get, import, joined, near, prop_text};

// A square pyramid, as the format's program writes a stockpile: four base
// corners and an apex, four triangles. Indices in the file are ONE-based.
const PYRAMID: &str = r#"primitive_3d {
  name Stockpile
  model "Site Mesh"
  colour brown
  attributes { text "Material" "Crushed Rock" }
  trimesh_3d {
    info { flag 0 key 1 colour brown name Stockpile }
    vertices {
      0 0 0
      20 0 0
      20 20 0
      0 20 0
      10 10 10
    }
    faces { 1 2 5  2 3 5  3 4 5  4 1 5 }
  }
}"#;

mod mesh_import {
    use super::*;

    #[test]
    fn a_trimesh_becomes_a_mesh_in_the_session_with_its_name_layer_colour_and_attributes() {
        let domain = import(PYRAMID);
        assert_eq!(domain.meshes.len(), 1);
        let mesh = &domain.meshes[0];
        assert_eq!(mesh.name, "Stockpile");
        assert_eq!(mesh.layer, "Site Mesh", "an archive model is a layer path");
        assert_eq!(mesh.colour_name, "brown");
        assert_eq!(mesh.color, standard_colour("brown"));
        assert_eq!(prop_text(&mesh.properties, "Material"), "Crushed Rock");

        // Five vertices, four triangles; the apex is 10 above a 20 x 20 base.
        assert_eq!(mesh.mesh.vertices.len(), 5);
        assert_eq!(mesh.mesh.triangle_count(), 4);
        assert_eq!(mesh.mesh.vertices[4], Point3::new(10.0, 10.0, 10.0));
        // One-based in the file, zero-based here: face 1 is vertices 1, 2 and
        // 5.
        assert_eq!(mesh.mesh.faces[0], [0, 1, 4]);
        let box3 = mesh.mesh.bounds();
        assert_eq!(box3.min, Point3::new(0.0, 0.0, 0.0));
        assert_eq!(box3.max, Point3::new(20.0, 20.0, 10.0));
        // Each side of the pyramid spans a 20 m base edge and rises 10 m over
        // the 10 m to the middle: half of 20 by sqrt(10^2 + 10^2) = 141.42...
        near(mesh.mesh.area(), 4.0 * 0.5 * 20.0 * 200.0_f64.sqrt(), 1e-9);
        // Its footprint is the base square.
        assert_eq!(mesh.mesh.plan_hull().len(), 4);
        assert!(domain.warnings.is_empty(), "{}", joined(&domain.warnings));
    }

    #[test]
    fn per_face_colours_come_from_the_one_based_flags_into_the_info_table() {
        // Manual 1.4.9: face_flags index face_infos from 1, and 0 means none.
        let domain = import(
            r#"primitive_3d { name Painted
  trimesh_3d {
    vertices { 0 0 0  1 0 0  1 1 0  0 1 0 }
    faces { 1 2 3  1 3 4 }
    face_infos { 1 255 red first  2 255 blue second }
    face_flags { 2 1 }
  } }"#,
        );
        assert_eq!(domain.meshes.len(), 1);
        let mesh = &domain.meshes[0];
        assert_eq!(mesh.face_colour_names.len(), 2);
        assert_eq!(
            mesh.face_colour_names[0], "blue",
            "flag 2 is the SECOND info"
        );
        assert_eq!(mesh.face_colour_names[1], "red");
        assert_eq!(mesh.face_colors[0], standard_colour("blue"));
        assert_eq!(mesh.face_colors[1], standard_colour("red"));
    }

    #[test]
    fn a_mesh_with_no_coloured_face_has_no_table_rather_than_a_table_of_blanks() {
        let domain = import(
            r#"primitive_3d { name Plain
  trimesh_3d { vertices { 0 0 0  1 0 0  1 1 0 } faces { 1 2 3 } } }"#,
        );
        assert_eq!(domain.meshes.len(), 1);
        assert!(
            domain.meshes[0].face_colour_names.is_empty(),
            "a table read by position with every entry blank is not a table"
        );
        assert!(domain.meshes[0].face_colors.is_empty());
    }

    #[test]
    fn a_face_that_repeats_a_vertex_or_uses_one_with_no_height_is_dropped_and_said_so() {
        // Three sound faces, and two that name a point in space they cannot:
        // one repeats a vertex, one uses a vertex with no height (the
        // format's null).
        let domain = import(
            r#"primitive_3d { name Broken
  trimesh_3d {
    vertices { 0 0 0  1 0 0  1 1 0  0 1 0  2 2 null }
    faces { 1 2 3  1 3 4  2 3 4  1 1 2  1 2 5 }
  } }"#,
        );
        assert_eq!(domain.meshes.len(), 1);
        assert_eq!(
            domain.meshes[0].mesh.triangle_count(),
            3,
            "the three sound faces"
        );
        assert!(
            validate(&domain.meshes[0].mesh).is_ok(),
            "what is imported is always valid"
        );
        assert!(
            any_contains(&domain.warnings, "2 mesh faces were dropped"),
            "{}",
            joined(&domain.warnings)
        );
    }

    #[test]
    fn a_face_naming_a_vertex_that_does_not_exist_is_refused_by_the_reader_not_the_import() {
        // The check belongs where it can say which line: an index past the end
        // is a broken FILE, not a face to drop quietly.
        let error = read_archive(
            r#"primitive_3d { name Broken
  trimesh_3d { vertices { 0 0 0  1 0 0  1 1 0 } faces { 1 2 99 } } }"#,
            &ReadOptions::default(),
        )
        .expect_err("vertex 99 does not exist");
        assert!(
            error.describe().contains("names point 99"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_mesh_with_no_sound_face_is_not_imported_and_is_counted() {
        // Every vertex null: nothing of it stands anywhere in space.
        let domain = import(
            r#"primitive_3d { name Hopeless
  trimesh_3d { vertices { 0 0 null  1 0 null  1 1 null } faces { 1 2 3 } } }"#,
        );
        assert!(domain.meshes.is_empty());
        assert!(
            any_contains(&domain.warnings, "1 trimeshes have no triangle"),
            "{}",
            joined(&domain.warnings)
        );
        let tally = domain
            .tally
            .iter()
            .find(|tally| tally.keyword == "primitive_3d")
            .expect("a tally of the trimeshes");
        assert_eq!(tally.read, 1);
        assert_eq!(tally.imported, 0);
    }
}

mod mesh_export {
    use super::*;

    #[test]
    fn a_mesh_goes_back_out_as_a_primitive3d_with_its_faces_one_based_again() {
        let first = import(PYRAMID);
        assert_eq!(first.meshes.len(), 1);
        let out = ExportMesh {
            name: first.meshes[0].name.clone(),
            layer: first.meshes[0].layer.clone(),
            colour_name: first.meshes[0].colour_name.clone(),
            mesh: Some(&first.meshes[0].mesh),
            face_colour_names: Vec::new(),
        };

        let empty = Model::default();
        let exported = from_domain(&empty, &[], &ExportOptions::default(), &[out])
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(exported.meshes_written, 1);

        // Through the writer and back: the same mesh.
        let back = read_archive(
            write_archive(&exported.archive, &WriteOptions::default()),
            &ReadOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let again = to_domain(&back, &ImportOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(again.meshes.len(), 1);
        let round = &again.meshes[0];
        assert_eq!(round.name, "Stockpile");
        assert_eq!(round.layer, "Site Mesh");
        assert_eq!(round.colour_name, "brown");
        assert_eq!(round.mesh.vertices, first.meshes[0].mesh.vertices);
        assert_eq!(round.mesh.faces, first.meshes[0].mesh.faces);
    }

    #[test]
    fn face_colours_become_one_info_per_distinct_colour_and_flags_that_index_it() {
        let mut mesh = TriangleMesh {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![[0, 1, 2], [0, 2, 3]],
        };
        let empty = Model::default();
        {
            let out = ExportMesh {
                name: String::from("Painted"),
                mesh: Some(&mesh),
                // Both faces red: one info, not two, because an info is a
                // colour and not a face.
                face_colour_names: vec![String::from("red"), String::from("red")],
                ..ExportMesh::default()
            };
            let exported = from_domain(&empty, &[], &ExportOptions::default(), &[out])
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            let written = get::<Trimesh>(&exported.archive.elements[0]);
            assert_eq!(written.face_infos.len(), 1);
            assert_eq!(written.face_infos[0].colour, "red");
            assert_eq!(written.face_flags, vec![1, 1]);
            // And one-based indices in the file itself.
            assert!(write_archive(&exported.archive, &WriteOptions::default()).contains("faces"));
        }

        // Two colours and one uncoloured face: two infos, and a 0 for the face
        // that has none.
        mesh.faces.push([1, 2, 3]);
        let out = ExportMesh {
            name: String::from("Painted"),
            mesh: Some(&mesh),
            face_colour_names: vec![String::from("red"), String::from("blue"), String::new()],
            ..ExportMesh::default()
        };
        let second = from_domain(&empty, &[], &ExportOptions::default(), &[out])
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let twice = get::<Trimesh>(&second.archive.elements[0]);
        assert_eq!(twice.face_infos.len(), 2);
        assert_eq!(twice.face_flags, vec![1, 2, 0]);
    }
}

// A `polymesh` inside a trimesh_3d, as the format's program exports one: the
// mesh's triangles grouped into the polygonal faces they make, each with its
// own info, its triangles numbered from 1 (face_indices), and its boundary
// and holes as loops of vertices. Invented here: a 2 x 2 square of two
// triangles whose one polygonal face is "Top", plus a second face naming a
// triangle 7 the mesh does not have. Before 2026-11-19 the block was skipped
// and said as "not read: primitive_3d/trimesh_3d/polymesh".
const WITH_POLYMESH: &str = r#"primitive_3d {
  name Pavement
  colour red
  trimesh_3d {
    vertices {
      0 0 1
      2 0 1
      2 2 1
      0 2 1
    }
    faces { 1 2 3  1 3 4 }
    polymesh {
      info {
        flag 0
        key 255
        colour "no colour"
        name ""
      }
      faces {
        face {
          valid true
          info {
            flag 0
            key 255
            colour "acad 040"
            name "Top"
          }
          normal 0 0 1
          face_indices { 1 2 }
          edge_indices { 1 2 3 4 }
          boundary {
            centroid 1 1 1
            radius 0
            vertex_indices { 1 2 3 4 }
          }
          holes {
            hole {
              centroid 1 1 1
              radius 0
              vertex_indices { 1 3 }
            }
          }
        }
        face {
          valid true
          face_indices { 2 7 }
        }
      }
      edge_indices { 1 2 3 4 5 }
    }
  }
}"#;

mod polymesh {
    use super::*;
    use katana_archive12d::{Polymesh, PolymeshFace, PolymeshLoop, TrimeshInfo};

    fn top_info() -> TrimeshInfo {
        TrimeshInfo {
            flag: 0,
            key: 255,
            colour: String::from("acad 040"),
            name: String::from("Top"),
        }
    }

    #[test]
    fn a_polymesh_is_read_as_the_faces_its_triangles_make_and_not_reported_unread() {
        let archive = match read_archive(WITH_POLYMESH, &ReadOptions::default()) {
            Ok(archive) => archive,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(
            archive.unrecognised.is_empty(),
            "{:?}",
            archive.unrecognised
        );
        let mesh = get::<Trimesh>(&archive.elements[0]);
        // The triangles are the mesh's own, read from faces as before.
        assert_eq!(mesh.faces, vec![[0, 1, 2], [0, 2, 3]]);
        let polymesh = mesh.polymesh.as_ref().expect("the polymesh is read");
        assert_eq!(polymesh.info.as_ref().map(|info| info.key), Some(255));
        assert_eq!(polymesh.edge_indices, vec![1, 2, 3, 4, 5]);
        assert_eq!(polymesh.faces.len(), 2);
        let top = &polymesh.faces[0];
        assert_eq!(top.valid, "true");
        assert_eq!(top.info, Some(top_info()));
        assert_eq!(top.normal, Some([0.0, 0.0, 1.0]));
        // Numbered from 1 in the file, zero-based here: both triangles.
        assert_eq!(top.triangles, vec![0, 1]);
        assert_eq!(top.edge_indices, vec![1, 2, 3, 4]);
        let boundary = top.boundary.as_ref().expect("the boundary");
        assert_eq!(boundary.centroid, Some([1.0, 1.0, 1.0]));
        assert_eq!(boundary.radius, Some(0.0));
        assert_eq!(boundary.vertices, vec![0, 1, 2, 3]);
        assert_eq!(top.holes.len(), 1);
        assert_eq!(top.holes[0].vertices, vec![0, 2]);
        // Triangle 7 is not the mesh's: left out, and said.
        assert_eq!(polymesh.faces[1].triangles, vec![1]);
        assert!(
            any_contains(&archive.warnings, "names 1 triangles or vertices"),
            "{}",
            joined(&archive.warnings)
        );
    }

    #[test]
    fn a_polymesh_written_reads_back_as_it_was_read() {
        let first = match read_archive(WITH_POLYMESH, &ReadOptions::default()) {
            Ok(archive) => archive,
            Err(error) => panic!("{}", error.describe()),
        };
        let text = write_archive(&first, &WriteOptions::default());
        let second = match read_archive(&text, &ReadOptions::default()) {
            Ok(archive) => archive,
            Err(error) => panic!("{}\n{text}", error.describe()),
        };
        assert!(second.unrecognised.is_empty(), "{:?}", second.unrecognised);
        let expected = Polymesh {
            info: Some(TrimeshInfo {
                flag: 0,
                key: 255,
                colour: String::from("no colour"),
                name: String::new(),
            }),
            faces: vec![
                PolymeshFace {
                    valid: String::from("true"),
                    info: Some(top_info()),
                    normal: Some([0.0, 0.0, 1.0]),
                    triangles: vec![0, 1],
                    edge_indices: vec![1, 2, 3, 4],
                    boundary: Some(PolymeshLoop {
                        centroid: Some([1.0, 1.0, 1.0]),
                        radius: Some(0.0),
                        vertices: vec![0, 1, 2, 3],
                    }),
                    holes: vec![PolymeshLoop {
                        centroid: Some([1.0, 1.0, 1.0]),
                        radius: Some(0.0),
                        vertices: vec![0, 2],
                    }],
                },
                PolymeshFace {
                    valid: String::from("true"),
                    triangles: vec![1],
                    ..PolymeshFace::default()
                },
            ],
            edge_indices: vec![1, 2, 3, 4, 5],
        };
        assert_eq!(
            get::<Trimesh>(&second.elements[0]).polymesh,
            Some(expected),
            "{text}"
        );
    }

    #[test]
    fn a_mesh_with_a_polymesh_imports_every_triangle_and_says_the_grouping_is_not_drawn() {
        let domain = import(WITH_POLYMESH);
        assert_eq!(domain.meshes.len(), 1);
        assert_eq!(domain.meshes[0].mesh.triangle_count(), 2);
        assert_eq!(domain.meshes[0].polymesh_faces_in_file, 2);
        assert!(
            any_contains(&domain.warnings, "1 meshes group their triangles into 2 polymesh faces"),
            "{}",
            joined(&domain.warnings)
        );
        assert!(
            !any_contains(&domain.warnings, "not read"),
            "{}",
            joined(&domain.warnings)
        );
    }
}
