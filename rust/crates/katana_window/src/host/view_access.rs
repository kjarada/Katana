//! The workspace's views, as a dialog reaches them: the calls the geo, survey, utility, online and
//! customisation dialogs and zoom-to make on ViewWorkspace (the plan's section 2), so a dialog
//! depends on this and not on the workspace (WS, wave 3), and a test hands a dialog a fake.
//!
//! The calls, by use: `viewSet()` (a scope's VIEW, ten dialogs), `zoomTo(box)`,
//! `zoomExtentsAll()`, `zoomToSelection(view)`, `activePlanView()`, `planViews()` (a point pick),
//! and `refreshAll()`. The surface display's `setSurfaces` is the window's own (K's) and not here.
//!
//! A point picked in a plan view (geo/point_pick.hpp, a sample point, a viewshed's observer) is a
//! request and an answer, since an immediate-mode dialog cannot wait on a callback across frames:
//! the dialog asks (`pick_point`) and reads the answer in a later frame (`picked_point`). The next
//! left click in any plan view is the point, in ground units, read through that view's own
//! transform; a right click or Esc gives no point; asking again ends the first pick with no point.

use std::cell::RefCell;
use std::rc::Rc;

use katana_cad::view_set::{ViewId, ViewSet};
use katana_geometry::{Box2, Point2};

/// A point pick asked for, to read its answer by.
pub type PickId = u64;

/// ViewWorkspace, as the dialogs call it.
pub trait ViewAccess {
    /// The open views (ViewWorkspace::viewSet), shared with the interpreter's hooks as the window
    /// keeps them (the plan's section 4.2). Borrow it for the read and let it go.
    fn view_set(&self) -> Rc<RefCell<ViewSet>>;
    /// Frames `area` in the plan views (ViewWorkspace::zoomTo).
    fn zoom_to(&mut self, area: &Box2);
    /// Zoom Extents in every view.
    fn zoom_extents_all(&mut self);
    /// Frames the selection in view `view`.
    fn zoom_to_selection(&mut self, view: ViewId);
    /// The plan view the menus act on (ViewWorkspace::activePlanView): its id, None when no plan
    /// view is open. Its state - its visible area - is in `view_set`.
    fn active_plan_view(&self) -> Option<ViewId>;
    /// Every open plan view, in the workspace's order.
    fn plan_views(&self) -> Vec<ViewId>;
    /// Repaints every view.
    fn refresh_all(&mut self);
    /// Asks for the next left click in a plan view; a pick waiting before this one ends with no
    /// point. With no plan view open, the answer is there at once: no point.
    fn pick_point(&mut self) -> PickId;
    /// The answer to `pick`, taken once: None while it is waiting, Some(None) for no point.
    fn picked_point(&mut self, pick: PickId) -> Option<Option<Point2>>;
}
