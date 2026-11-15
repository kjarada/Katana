//! tests/geo/test_formats.cpp: its one case that drives the executor - a
//! GeoPackage GDAL writes holding only a raster routes to the raster import.
//! The file's FormatsVerb suite is the frame's (tests/geo_frame), its other
//! Formats cases interop's (step 5).

mod formats {
    use std::path::Path;

    use katana_gis::formats::identify_content;
    use katana_interop::import::{import_raster, kind_for_path};
    use katana_interop::{RasterImportOptions, SourceKind};

    use crate::geo_fixture::{GeoFixture, geo_data, must, near, shared_data};

    #[test]
    fn a_gpkg_holding_only_a_raster_routes_to_raster_import() {
        let _shared = shared_data();
        // GDAL writes the plane's grid as the GeoPackage's tiles; the name
        // alone says vector data, the content says raster.
        let mut fixture = GeoFixture::new("katana-formats-", "gpkg-raster");
        let gpkg = fixture.scratch.file("plane.gpkg");
        must(fixture.run(&format!(
            "GDAL raster convert \"{}/plane.asc\" \"{gpkg}\" --output-format GPKG",
            geo_data()
        )));

        let content = must(identify_content(Path::new(&gpkg)));
        assert_eq!(content.driver, "GPKG");
        assert!(content.raster);
        assert!(!content.vector);
        assert_eq!(kind_for_path(Path::new(&gpkg)), SourceKind::Raster);
        assert_eq!(
            kind_for_path(&fixture.scratch.path().join("not-written.gpkg")),
            SourceKind::Vector,
            "only a name to go by: a GeoPackage is read as features"
        );

        let raster = must(import_raster(
            Path::new(&gpkg),
            &RasterImportOptions::default(),
        ));
        // plane.asc: 40 x 30 cells of 1 m from (0,0).
        assert_eq!(raster.width, 40);
        assert_eq!(raster.height, 30);
        near(raster.geotransform[0], 0.0, 1e-9);
        near(raster.geotransform[3], 30.0, 1e-9);
    }
}
