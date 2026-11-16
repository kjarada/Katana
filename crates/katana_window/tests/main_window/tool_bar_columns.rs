//! The side columns' toolbars (main_window::layout::column_rooms): every bar its length while the
//! column has room; else the bar that does not fit takes the rest, but every bar after it keeps
//! the room of its overflow arrow, so no bar of the column is left out with its tools out of reach.

use katana_window::main_window::layout::{collapsed_bar, column_rooms};

mod tool_bar_columns {
    use super::*;

    /// An arrow's room, as the tests reason in round numbers.
    const ARROW: f32 = 24.0;

    #[test]
    fn a_column_with_room_gives_every_bar_its_length() {
        assert_eq!(column_rooms(&[400, 150], 700.0, ARROW), [400.0, 150.0]);
        // Exactly the room: still every bar its length.
        assert_eq!(column_rooms(&[400, 150], 550.0, ARROW), [400.0, 150.0]);
    }

    #[test]
    fn a_bar_under_one_that_does_not_fit_keeps_the_room_of_its_arrow() {
        // At 1360 x 860 the left column has some 650 points: Draw, longer than that, took all of
        // it and Annotate under it was not drawn (the second look audit's T18). Draw now gives up
        // the room of Annotate's arrow, 650 - 24.
        assert_eq!(column_rooms(&[900, 200], 650.0, ARROW), [626.0, 24.0]);
        // Three bars: the two after the first keep an arrow's room each.
        assert_eq!(
            column_rooms(&[900, 200, 350], 700.0, ARROW),
            [652.0, 24.0, 24.0]
        );
        // The first fits, the second does not: the second takes the rest, as in a row.
        assert_eq!(column_rooms(&[300, 500], 600.0, ARROW), [300.0, 300.0]);
    }

    #[test]
    fn no_room_gives_nothing_and_no_bars_need_none() {
        assert_eq!(column_rooms(&[900, 200], 0.0, ARROW), [0.0, 0.0]);
        assert_eq!(column_rooms(&[900, 200], -10.0, ARROW), [0.0, 0.0]);
        assert!(column_rooms(&[], 600.0, ARROW).is_empty());
        // Less than an arrow: the first bar has it, as a row would give it.
        assert_eq!(column_rooms(&[900, 200], 10.0, ARROW), [10.0, 0.0]);
    }

    #[test]
    fn a_collapsed_bar_is_one_tool_button_and_its_padding() {
        // tools::flyout_button: 6 of padding at either end of a strip and a tool button, its
        // icon, Qt's 3, a padding of 4 and a border of 1 each side - 33 at a toolbar's 20 px
        // icons. It was the 12 point overflow arrow, 24 in all, and under Draw's own arrow the
        // two read as one sign drawn twice (the third look audit's G); the bar is now drawn as
        // its first tool with a family's corner mark, which opens its tools.
        assert_eq!(collapsed_bar(20), 6.0 + 33.0 + 6.0);
        assert_eq!(collapsed_bar(16), 6.0 + 29.0 + 6.0);
        assert_eq!(collapsed_bar(28), 6.0 + 41.0 + 6.0);
    }
}
