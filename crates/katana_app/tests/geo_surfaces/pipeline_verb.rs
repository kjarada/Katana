//! tests/geo/test_pipeline_verb.cpp: a GDAL pipeline through the executor
//! (docs/geoprocessing.md, "Pipelines"), its result bound as a run's output
//! unless its last step prints - the cases that keep a raster as a reference
//! raster or read the reference data. Values by hand on
//! tests/geo/data/plane.asc: 40 x 30 cells of 1 m, z = 100 + 0.05x at the
//! centres, so from 100.025 to 101.975. Its other PipelineVerb cases are
//! G1's (tests/geo_frame); its PipelineContract suite is step 5's
//! (katana_gis).

mod pipeline_verb {
    use crate::geo_fixture::{GeoFixture, holds, shared_data};
    use crate::surface_support::plane;

    /// The C++ fixture's: one directory for each test, emptied before and
    /// after (here: removed as the fixture is made, and with it) - a raster an
    /// earlier run left there would take the name a test expects.
    fn fixture(name: &str) -> GeoFixture {
        let fixture = GeoFixture::new("katana-pipeline-verb-", name);
        let _ = std::fs::remove_dir_all(fixture.scratch.path());
        fixture
    }

    #[test]
    fn a_pipeline_ending_in_write_keeps_its_raster_as_a_reference() {
        let _shared = shared_data();
        let mut fixture = fixture("APipelineEndingInWriteKeepsItsRasterAsAReference");
        let reply = fixture.ok(&format!(
            "GDAL pipeline \"read ! hillshade ! write\" FROM input FILE \"{}\" TO REFERENCE shade",
            plane()
        ));
        holds(&reply, "target=reference id=1 name=shade raster=40x30");
        assert_eq!(fixture.reference.rasters().len(), 1);
    }

    #[test]
    fn a_pipeline_ending_in_info_prints() {
        let _shared = shared_data();
        let mut fixture = fixture("APipelineEndingInInfoPrints");
        let reply = fixture.ok(&format!(
            "GDAL pipeline \"read ! info\" FROM input FILE \"{}\"",
            plane()
        ));
        holds(&reply, "text lines=");
        assert!(fixture.reference.rasters().is_empty());
    }
}
