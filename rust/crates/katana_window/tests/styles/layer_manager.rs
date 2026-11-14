//! The twins of tests/qt_widgets/customisation/test_layer_manager.cpp: the Layers dialog's half of
//! audit QT-02 - a layer naming a library linestyle no loaded library defines, and a hatch pattern
//! and dimension style the model lacks, keeps every name through Save, and an unedited Save is no
//! command.

use katana_commands::create_layer;
use katana_entity::{Color, Layer};
use katana_window::host::RecordingHost;
use katana_window::layer_manager::LayerManagerDialog;

use crate::support::{Bench, must};

/// Layer "survey" as an archive import leaves it when its customisation is not loaded: a linetype,
/// a hatch pattern and a dimension style that nothing in this drawing defines, and a weight of
/// 0.125 mm, which the dialog's two-decimal weight box shows as 0.13 (0.125 rounds half away from
/// zero).
fn survey() -> Layer {
    Layer {
        name: String::from("survey"),
        linetype: String::from("Old Survey Kerb"),
        hatch_pattern: String::from("Old Survey Hatch"),
        dimension_style: String::from("Old Survey Dimensions"),
        line_weight: 0.125,
        color: Color::new(0, 200, 120, 255),
        ..Layer::default()
    }
}

fn fixture() -> Bench<LayerManagerDialog> {
    let mut host = RecordingHost::new();
    must(&mut host, create_layer(survey()));
    Bench::new(host, LayerManagerDialog::new)
}

fn stored(bench: &Bench<LayerManagerDialog>) -> Layer {
    bench
        .document()
        .model()
        .layers
        .find("survey")
        .expect("survey is there")
        .clone()
}

mod layer_dialog {
    use super::*;

    #[test]
    fn saving_a_layer_naming_undefined_names_unedited_keeps_every_name_and_pushes_nothing() {
        let mut bench = fixture();
        bench.fill("layerTable", "survey");
        let steps = bench.steps();
        bench.press("layerSave");

        assert_eq!(stored(&bench).linetype, "Old Survey Kerb");
        assert_eq!(stored(&bench).hatch_pattern, "Old Survey Hatch");
        assert_eq!(stored(&bench).dimension_style, "Old Survey Dimensions");
        assert_eq!(stored(&bench).line_weight, 0.125);
        assert_eq!(stored(&bench), survey());
        assert_eq!(bench.steps(), steps);

        // The field shows the stored name, marked, not a neighbour's.
        assert_eq!(bench.dialog.linetype().current_name(), "Old Survey Kerb");
        assert!(!bench.dialog.linetype().current_is_defined());
    }

    #[test]
    fn editing_the_weight_alone_saves_it_and_still_keeps_the_undefined_names() {
        let mut bench = fixture();
        bench.fill("layerTable", "survey");
        bench.fill("layerWeight", "0.5");
        let steps = bench.steps();
        bench.press("layerSave");

        assert_eq!(bench.steps(), steps + 1);
        assert_eq!(stored(&bench).line_weight, 0.5);
        assert_eq!(stored(&bench).linetype, "Old Survey Kerb");
        assert_eq!(stored(&bench).hatch_pattern, "Old Survey Hatch");
        assert_eq!(stored(&bench).dimension_style, "Old Survey Dimensions");
        assert_eq!(stored(&bench).color, survey().color);
    }
}
