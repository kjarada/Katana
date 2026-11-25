//! DWG (docs/dwg.md): the window routes a .dwg as it routes a .dxf - File > Import's placement
//! step, before GDAL is asked what the file is (a GDAL with a CAD driver would take it for GIS
//! features). The step's line is IMPORT's, which the executor sends to katana_dwg.

use std::path::Path;

use katana_window::MainWindow;

mod dwg_routing {
    use super::*;

    #[test]
    fn a_dwg_opens_the_placement_step_a_dxf_opens_by_its_extension_alone() {
        let mut window = MainWindow::new();
        // Not there, and not looked at: the step is chosen by the extension, in any case.
        for path in ["C:/data/site plan.dwg", "C:/data/SITE.DWG"] {
            let opened = window.open_import_options(Path::new(path));
            assert_eq!(opened.as_deref(), Some("importPlacementDialog"), "{path}");
        }
        // The control: a DXF's step is the same one.
        assert_eq!(
            window
                .open_import_options(Path::new("C:/data/site.dxf"))
                .as_deref(),
            Some("importPlacementDialog")
        );
    }
}
