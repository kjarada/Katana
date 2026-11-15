//! When the keyboard focus arriving in a view makes it the active view, the one the menus, Zoom
//! Extents and F9 act on. A click always does (each view's own press); the focus does only when the
//! USER moved it. One rule for the plan, 3D and section views, because the workspace treats them
//! alike.
//!
//! The C++ answers from Qt's focus reason and the widget the focus came from
//! (`QApplication::focusChanged` with the reason a FocusIn event carried, `activateOnFocus`). egui
//! keeps no reason, so the workspace (WS) says which way the focus came - the facts below - and
//! this is the rule over them, as the C++ wrote it.

/// Why the focus moved, as Qt names its reasons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FocusReason {
    /// A press of the mouse.
    Mouse,
    /// Tab, along the tab chain.
    Tab,
    /// Shift+Tab.
    Backtab,
    /// A window coming forward gives the focus back to the widget that last held it.
    ActiveWindow,
    /// A closing menu gives it back to the view under it.
    Popup,
    /// A shortcut.
    Shortcut,
    /// Leaving the menu bar by the keyboard.
    MenuBar,
    /// The application giving the view focus, or anything else.
    #[default]
    Other,
}

/// The widget the focus came from, as far as the rule asks about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FocusPrevious {
    pub visible: bool,
    pub enabled: bool,
}

/// Whether focus arriving in a view for `reason`, from `previous` (None: no widget held it), is the
/// user choosing that view. `view_floats` is whether the view's window is a floating dock - a
/// window with a parent (the workspace) - rather than the main window.
pub fn focus_chooses_view(
    reason: FocusReason,
    previous: Option<FocusPrevious>,
    view_floats: bool,
) -> bool {
    match reason {
        // Qt moves the focus along the tab chain ITSELF when the widget holding it is hidden or
        // disabled, and the workspace hides a view whenever it changes the view's kind, closes it
        // or rearranges the docks. Whichever view the chain reached next became active: the menus
        // then acted on an unrelated view, and a close overruled ViewSet's "most recently active
        // of the rest". Tab pressed in a view that is still there is a choice.
        FocusReason::Tab | FocusReason::Backtab => {
            previous.is_none_or(|previous| previous.visible && previous.enabled)
        }
        // A floating view's own window coming forward is the user choosing that view. The main
        // window comes forward whenever the user returns to it - after a dialog, or to reach its
        // menu bar from a floating view - and the view holding its focus was not chosen: with it,
        // a floating view could never be what the menu bar acted on.
        FocusReason::ActiveWindow => view_floats,
        FocusReason::Popup | FocusReason::MenuBar => false,
        // A click, a shortcut, or the application giving the view focus.
        FocusReason::Mouse | FocusReason::Shortcut | FocusReason::Other => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{FocusPrevious, FocusReason, focus_chooses_view};

    // The rule's table, each reason once, worked from the C++'s switch.
    #[test]
    fn each_reason_chooses_the_view_as_the_cpp_switch_says() {
        let gone = FocusPrevious {
            visible: false,
            enabled: true,
        };
        let there = FocusPrevious {
            visible: true,
            enabled: true,
        };
        assert!(focus_chooses_view(FocusReason::Tab, None, false));
        assert!(focus_chooses_view(FocusReason::Tab, Some(there), false));
        assert!(!focus_chooses_view(FocusReason::Backtab, Some(gone), false));
        assert!(focus_chooses_view(FocusReason::ActiveWindow, None, true));
        assert!(!focus_chooses_view(FocusReason::ActiveWindow, None, false));
        assert!(!focus_chooses_view(FocusReason::Popup, None, true));
        assert!(!focus_chooses_view(FocusReason::MenuBar, None, true));
        assert!(focus_chooses_view(FocusReason::Mouse, Some(gone), false));
        assert!(focus_chooses_view(FocusReason::Other, None, false));
    }
}
