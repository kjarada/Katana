//! tests/geo/test_gdal_verbs.cpp: the GeoExecutor cases that read the
//! reference data, keep a raster as a reference raster or bind a RASTER or
//! SURFACE source, and its GeoSession case. Its other GeoExecutor cases are
//! G1's (tests/geo_frame).

mod geo_executor {
    use std::path::Path;
    use std::sync::Arc;

    use katana_core::ErrorCode;
    use katana_geometry::Point3;
    use katana_gis::GdalDataset;
    use katana_interop::RasterRole;
    use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

    use crate::geo_fixture::{GeoFixture, holds, must, near, refused, shared_data};
    use crate::surface_support::{code, data};

    /// The C++ fixture's: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-geo-verbs-", name)
    }

    #[test]
    fn a_file_only_gdal_line_writes_the_file() {
        let _shared = shared_data();
        let mut fixture = fixture("AFileOnlyGdalLineWritesTheFile");
        let out = fixture.scratch.file("shade.tif");
        let reply = must(fixture.run(&format!(
            "GDAL raster hillshade \"{}/plane.asc\" \"{out}\" --zfactor 2",
            data()
        )));
        holds(&reply, "kind=file target=file");
        assert!(Path::new(&out).exists());
        assert!(fixture.reference.rasters().is_empty());
        assert_eq!(fixture.document.model().entities.size(), 0);
    }

    #[test]
    fn a_raster_result_becomes_a_derived_reference_raster() {
        let _shared = shared_data();
        let mut fixture = fixture("ARasterResultBecomesADerivedReferenceRaster");
        let line = format!("GDAL raster hillshade \"{}/plane.asc\"", data());
        let reply = must(fixture.run(&line));
        assert_eq!(fixture.reference.rasters().len(), 1);
        let shade = &fixture.reference.rasters()[0];
        assert_eq!(shade.name, "hillshade");
        assert_eq!(shade.role, RasterRole::Derived);
        assert_eq!(shade.derivation, line);
        assert_eq!(shade.width, 40);
        // No project: kept in the scratch folder, and said so.
        holds(&reply, "persisted=no");
        assert_eq!(shade.source.parent(), Some(fixture.scratch.path()));
        // A second takes the next free name.
        must(fixture.run(&line));
        assert_eq!(fixture.reference.rasters().len(), 2);
        assert_eq!(fixture.reference.rasters()[1].name, "hillshade-2");
    }

    // A source of a kind the argument does not read is refused as the line is
    // read, before GDAL is given anything: hillshade's input reads rasters
    // (GDAL HELP says kinds=raster), so the drawing is refused, previewed or
    // run, and nothing is drawn or kept; buffer's reads vectors, so a raster
    // is refused. GDAL itself answered the first only "Unable to fetch band
    // #1", from the worker.
    #[test]
    fn a_source_of_a_kind_the_argument_does_not_read_is_refused_before_it_runs() {
        let _shared = shared_data();
        let mut fixture = fixture("ASourceOfAKindTheArgumentDoesNotReadIsRefusedBeforeItRuns");
        fixture.type_line("RECT 0,0 10,10");
        must(fixture.run(&format!("IMPORT \"{}/plane.asc\"", data())));
        let steps = fixture.document.history().undo_count();
        for line in [
            "GDAL raster hillshade FROM DRAWING",
            "GDAL raster hillshade FROM DRAWING PREVIEW",
            "GDAL raster hillshade FROM input LAYERS 0 TO REFERENCE shade",
            "GDAL vector buffer --distance=1 FROM RASTER plane",
            "GDAL vector buffer --distance=1 FROM input SURFACE nothing",
        ] {
            let reply = refused(fixture.run(line), line);
            assert_eq!(reply.code, ErrorCode::InvalidArgument, "{line}");
            assert_eq!(reply.context, "input", "{line}");
            assert!(
                reply.message.contains("; it takes "),
                "{line}: {}",
                reply.message
            );
        }
        assert_eq!(fixture.document.history().undo_count(), steps);
        assert_eq!(fixture.reference.rasters().len(), 1);
    }

    #[test]
    fn a_raster_source_is_the_reference_rasters_own_file() {
        let _shared = shared_data();
        let mut fixture = fixture("ARasterSourceIsTheReferenceRastersOwnFile");
        must(fixture.run(&format!("GDAL raster hillshade \"{}/plane.asc\"", data())));
        let reply = must(fixture.run("GDAL raster info --format=text FROM RASTER hillshade"));
        holds(&reply, "Size is 40, 30");
        holds(&reply, "input arg=input source=raster name=hillshade");
        assert_eq!(
            code(fixture.run("GDAL raster info FROM RASTER 99")),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_surface_is_sampled_at_its_cell_centres() {
        // Two triangles over 40 x 30 on z = 100 + 0.05 x: the plane of
        // plane.asc, so its slope is 5 % wherever the grid is inside it. The
        // TIN interpolates the plane exactly; the tolerance is the Float32
        // analysis of GdalRun.SlopeOfAPlaneRisingOneInTwentyIsFivePercent.
        let mut fixture = fixture("ASurfaceIsSampledAtItsCellCentres");
        let tin = must(TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 100.0),
                Point3::new(40.0, 0.0, 102.0),
                Point3::new(40.0, 30.0, 102.0),
                Point3::new(0.0, 30.0, 100.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        ));
        let mut store = SurfaceStore::default();
        must(store.add(NamedSurface {
            name: String::from("ground"),
            surface: Some(Arc::new(tin)),
            source: String::from("test"),
            ..NamedSurface::default()
        }));
        // The C++'s setSurfaces default step name.
        must(fixture.document.set_surfaces(store, "SET_SURFACES"));
        let reply = must(
            fixture
                .run("GDAL raster slope --unit=percent FROM SURFACE ground CELL 1 TO REFERENCE s"),
        );
        holds(&reply, "input arg=input source=surface name=ground cell=1");
        assert_eq!(fixture.reference.rasters().len(), 1);
        let slope = must(GdalDataset::open(&fixture.reference.rasters()[0].source));
        let band = must(slope.read_band(1));
        near(band[15 * 40 + 20], 5.0, 5e-4);
        assert_eq!(
            code(fixture.run("GDAL raster slope FROM SURFACE nowhere")),
            ErrorCode::NotFound
        );
    }
}

// ---- through a Session, as katana_cli and katana_mcp run it -------------------------------------

mod geo_session {
    use katana_app::{Captured, Session};

    use crate::geo_fixture::{holds, shared_data};
    use crate::surface_support::data;

    #[test]
    fn a_gdal_line_runs_through_the_session_and_its_raster_is_a_reference() {
        let _shared = shared_data();
        let mut session = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        assert!(session.run(
            format!("GDAL raster hillshade \"{}/plane.asc\"", data()),
            &mut captured
        ));
        assert!(session.run("REFS", &mut captured));
        holds(&captured.out, "target=reference id=1 name=hillshade");
        // REFS replies in records since IMPORT, EXPORT, INFO, REFS and COPC
        // became the executor's (docs/interop.md).
        holds(
            &captured.out,
            "reference id=1 kind=raster name=hillshade width=40 height=30",
        );
        assert!(!session.run("GDAL raster hillshade --config X=Y", &mut captured));
        holds(&captured.err, "error: InvalidArgument: --config is refused");
        holds(&Session::help_text(), "GDAL VERSION");
    }
}
