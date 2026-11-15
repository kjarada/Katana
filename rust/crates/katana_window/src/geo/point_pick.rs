//! A point picked in a plan view for a dialog (geo/point_pick.hpp; docs/geoprocessing.md, "The
//! window"; docs/terrain.md, "Sampling and drape"): the sample points, a viewshed's observers, a
//! sight line's ends.
//!
//! The next left click in any plan view of the workspace is the point, in ground units, read
//! through that view's own transform; the click is taken by the pick and selects nothing. A right
//! click or Esc in the view gives no point. Middle-button panning and the wheel pass through, so
//! the person can move the view to the place first. Asking again while a pick is waiting ends the
//! first with no point. What the pick hands back is only a point: the dialog writes it into its
//! line (AT x,y, OBSERVER x,y), so what runs is still a line an agent could type.
//!
//! The watching of the views is the workspace's (`ViewAccess::pick_point`, WS's): an immediate-mode
//! dialog cannot wait on a callback across frames, so it asks, and reads the answer in a later
//! frame. This is the dialog's side: a pick asked, and its answer taken once. With no views (a
//! test, a headless batch) the C++'s picker was unset and a dialog offered no Pick (`offered`).

use katana_geometry::Point2;

use crate::host::{Host, PickId};

/// A dialog's point pick: none, or one asked and waiting for its answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointPick {
    waiting: Option<PickId>,
}

impl PointPick {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a dialog offers Pick at all: only where there are views to click in.
    pub fn offered(host: &mut dyn Host) -> bool {
        host.views().is_some()
    }

    /// Asks for the next left click in a plan view, ending any pick of this dialog's still waiting.
    /// The answer may be there at once (no plan view open: no point); otherwise `poll` takes it in
    /// a later frame. With no views at all, no point.
    pub fn pick(&mut self, host: &mut dyn Host) -> Option<Option<Point2>> {
        let Some(views) = host.views() else {
            self.waiting = None;
            return Some(None);
        };
        self.waiting = Some(views.pick_point());
        self.poll(host)
    }

    /// The pick's answer, taken once: None while it is waiting, Some(None) for no point - a right
    /// click, Esc, another pick asked, or the views gone.
    pub fn poll(&mut self, host: &mut dyn Host) -> Option<Option<Point2>> {
        let pick = self.waiting?;
        let answer = match host.views() {
            Some(views) => views.picked_point(pick)?,
            None => None,
        };
        self.waiting = None;
        Some(answer)
    }

    /// Whether a pick is waiting for its click.
    pub fn waiting(&self) -> bool {
        self.waiting.is_some()
    }
}
