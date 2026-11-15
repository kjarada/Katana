//! The twin of tests/qt_widgets/plotting/test_legend_properties.cpp's summary case: the line under
//! a Legend viewport's scope choice says what the legend lists and where a fall-back led
//! (katana_window::plotting::legend_properties). Its other case chooses the scope in the sheet
//! editor's properties and waits for the sheet editor (SH2).

use katana_cad::plotting::legend::{Legend, LegendEntry};
use katana_cad::plotting::sheet_set::LegendScope;
use katana_window::plotting::legend_properties::legend_summary;

mod sheet_legend_properties {
    use super::*;

    #[test]
    fn the_summary_says_where_a_fall_back_led() {
        let mut legend = Legend {
            entries: vec![LegendEntry::default(); 3],
            windows: 2,
            scope: LegendScope::WholeSet,
            ..Legend::default()
        };
        assert_eq!(
            legend_summary(&legend, LegendScope::ThisSheet),
            "3 entries from 2 plans of the set - this sheet has no plan"
        );
        assert_eq!(
            legend_summary(&legend, LegendScope::WholeSet),
            "3 entries from 2 plans of the set"
        );
        legend.scope = LegendScope::WholeDrawing;
        legend.windows = 0;
        legend.entries.truncate(1);
        assert_eq!(
            legend_summary(&legend, LegendScope::ThisSheet),
            "1 entry from the whole drawing - no sheet has a plan"
        );
        assert_eq!(
            legend_summary(&legend, LegendScope::WholeDrawing),
            "1 entry from the whole drawing"
        );
    }
}
