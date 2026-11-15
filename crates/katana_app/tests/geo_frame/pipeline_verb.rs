//! tests/geo/test_pipeline_verb.cpp: a GDAL pipeline through the executor
//! (docs/geoprocessing.md, "Pipelines"), its result bound as a run's output
//! unless its last step prints. Values by hand on tests/geo/data/plane.asc:
//! 40 x 30 cells of 1 m, z = 100 + 0.05x at the centres, so from 100.025 to
//! 101.975.
//!
//! The PipelineVerb cases that keep a raster as a reference raster or read
//! the reference data (APipelineEndingInWriteKeepsItsRasterAsAReference,
//! APipelineEndingInInfoPrints) are step 6's wave 2 (A); the
//! PipelineContract suite is step 5's (katana_gis).

mod pipeline_verb {
    use crate::geo_fixture::{GeoFixture, geo_data, holds, shared_data};

    /// The C++ fixture's: one directory for each test, emptied before and
    /// after (here: removed as the fixture is made, and with it) - a raster an
    /// earlier run left there would take the name a test expects.
    fn fixture(name: &str) -> GeoFixture {
        let fixture = GeoFixture::new("katana-pipeline-verb-", name);
        let _ = std::fs::remove_dir_all(fixture.scratch.path());
        fixture
    }

    fn plane() -> String {
        format!("{}/plane.asc", geo_data())
    }

    #[test]
    fn a_pipeline_ending_in_write_draws_its_features_on_a_layer() {
        // Contours every 0.5 m over 100.025 .. 101.975: 100.5, 101.0 and
        // 101.5, each buffered into one area - three, by hand.
        let _shared = shared_data();
        let mut fixture = fixture("APipelineEndingInWriteDrawsItsFeaturesOnALayer");
        let reply = fixture.ok(&format!(
            "GDAL pipeline \"read ! contour --interval 0.5 ! buffer 0.1 ! write\" FROM input FILE \
             \"{}\" TO LAYER gis/bands",
            plane()
        ));
        holds(
            &reply,
            "output arg=output kind=vector target=layer layer=gis/bands created=3",
        );
        assert_eq!(fixture.document.model().entities.size(), 3);
    }

    // FROM without an argument's name binds the pipeline's one input, which
    // is optional (its read step may name a file): it was refused as "no
    // required dataset left", and docs/geoprocessing.md's own example with it
    // failed. The same three bands as the named FROM above.
    #[test]
    fn an_unnamed_from_binds_the_pipelines_one_input() {
        let _shared = shared_data();
        let mut fixture = fixture("AnUnnamedFromBindsThePipelinesOneInput");
        let reply = fixture.ok(&format!(
            "GDAL pipeline \"read ! contour --interval 0.5 ! buffer 0.1 ! write\" FROM FILE \
             \"{}\" TO LAYER gis/bands",
            plane()
        ));
        holds(&reply, "input arg=input source=file");
        holds(
            &reply,
            "output arg=output kind=vector target=layer layer=gis/bands created=3",
        );
    }
}
