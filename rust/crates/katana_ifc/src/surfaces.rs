//! Surfaces -> IfcGeographicElement TERRAIN.
//!
//! A TIN is written as an IfcTriangulatedFaceSet (Body, Tessellation): its
//! points once, its triangles as one-based triples of them. IFC 4.3 also has
//! IfcTriangulatedIrregularNetwork, a face set that can flag breaklines, but
//! the Alignment-based view this file declares does not include it (the
//! validation service's IFC431 lists it as out of that view's scope), and a
//! face set is what every viewer draws.

use std::collections::BTreeMap;

use katana_math::Vec3;

use crate::builder::{Builder, PropertyList};
use crate::classification::IfcClass;
use crate::export::SurfaceInput;
use crate::step::step_real;
use crate::step_file::{Args, Id, list_of};

pub(crate) fn export_surfaces(
    builder: &mut Builder<'_>,
    surfaces: &[SurfaceInput<'_>],
) -> BTreeMap<String, Id> {
    let mut written = BTreeMap::new();
    for input in surfaces {
        let Some(surface) = input.surface.filter(|surface| surface.triangle_count() > 0) else {
            builder.warn(format!(
                "surface \"{}\" has no triangles and is not written",
                input.name
            ));
            continue;
        };
        let mut coordinates = Vec::with_capacity(surface.vertex_count());
        for vertex in surface.vertices() {
            let at = builder.local_3d(Vec3::new(vertex.x, vertex.y, vertex.z));
            coordinates.push(list_of(&[
                step_real(at.x),
                step_real(at.y),
                step_real(at.z),
            ]));
        }
        let mut triangles = Vec::with_capacity(surface.triangle_count());
        for triangle in surface.triangles() {
            triangles.push(list_of(&[
                format!("{}", triangle[0] + 1),
                format!("{}", triangle[1] + 1),
                format!("{}", triangle[2] + 1),
            ]));
        }
        let points = builder.file().add(
            "IfcCartesianPointList3D",
            Args::new().raw(&list_of(&coordinates)).null(),
        );
        let faces = builder.file().add(
            "IfcTriangulatedFaceSet",
            Args::new()
                .reference(points)
                .null()
                .boolean(false)
                .raw(&list_of(&triangles))
                .null(),
        );
        let context = builder.body_context();
        let representation = builder.shape(context, "Body", "Tessellation", &[faces]);
        builder.layer(&format!("Surfaces/{}", input.name), representation);
        let key = format!("surface/{}", input.name);
        let terrain = IfcClass::new("IfcGeographicElement", "TERRAIN", "");
        let shape = builder.product_shape(&[representation]);
        let element = builder.product(&terrain, &key, &input.name, "Surface", shape, "");
        builder.tally(
            &format!("surface {}", input.name),
            &terrain,
            "",
            "a surface: its triangles",
        );
        let mut list = PropertyList::new();
        list.integer("Points", Some(surface.vertex_count() as i64));
        list.integer("Triangles", Some(surface.triangle_count() as i64));
        let set = builder.property_set(&key, "Katana_Surface", &list);
        builder.defines(&key, set, &[element]);
        builder.report().surfaces += 1;
        written.entry(input.name.clone()).or_insert(element);
    }
    written
}
