//! The open views of the workspace, and what each one is looking at (the
//! user's request of 2026-09-23: views that dock, float onto another screen,
//! minimise and close, each with its own layers; include/katana/cad/view_set.hpp,
//! src/katana_cad/view_set.cpp).
//!
//! This replaces the CELLS of the tiled ViewportLayout. A tile was identified by
//! its position in a fixed split, so a view's state lived as long as the split
//! did and every layout change rebuilt every widget - the plan zoom, the second
//! section and the half-picked clicks all went with it. A docked view is
//! identified by an id instead, lives until the user closes it, and can change
//! what it shows (plan, 3D, section, elevation) without losing the rest of its
//! state. The split presets survive as ARRANGEMENTS of these views
//! (viewport_layout.rs, dock_splits).
//!
//! Like ViewportLayout before it, this is the tested model: it owns no widgets
//! and draws nothing. The workspace hosts one dock per ViewState. States are
//! boxed, as the C++ holds them by unique_ptr, so a view's state never moves
//! while it is open whatever else opens or closes; a front end reaches it by
//! its id (find, find_mut), since Rust lends no reference across the set's
//! changes.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::text::{equals_ignoring_case, format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EnumName;
use katana_math::{to_metres, units};
use katana_render::Camera;

use crate::document::Document;
use crate::layer_overrides::LayerOverrides;
use crate::section::{Section, SectionDesign};
use crate::view_link::{follow_view, linkable};
use crate::view_transform::ViewTransform;
use crate::viewport_layout::{ViewKind, configure_camera};

/// Monotonic and never reused, like entity ids: a dock that outlives its view
/// by a queued event must not find a different view under the same id.
pub type ViewId = u32;
pub const NO_VIEW: ViewId = 0;

/// At the minimum camera zoom, one screen pixel represents 1e7 model metres.
/// A millimetre at this denominator reaches that same numerical envelope:
/// 1000 mm/m divided by ViewTransform's minimum pixels/m.
pub const MAXIMUM_PAPER_STYLE_SCALE: f64 = 1000.0 / ViewTransform::MINIMUM_SCALE;

const PAPER_SCALES_KEY: &str = "view_paper_scales";
const PAPER_SCALES_VERSION: i32 = 1;

/// None is Auto: paper-sized library marks keep their screen size. A fixed
/// denominator N makes each paper millimetre N/1000 model metres, independently
/// of the camera zoom, ANNOSCALE and the plot's scale.
pub fn validate_paper_style_scale(scale: Option<f64>) -> Status {
    if let Some(scale) = scale
        && (!scale.is_finite()
            || scale <= 0.0
            || scale > MAXIMUM_PAPER_STYLE_SCALE
            || to_metres(scale, units::MILLIMETRE) <= 0.0)
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "paper style scale must be Auto or a positive finite denominator (1:N), at most \
                 {}, with a nonzero millimetre in model units",
                format_exact_real(MAXIMUM_PAPER_STYLE_SCALE)
            ),
        ));
    }
    Ok(())
}

pub fn parse_paper_style_scale(text: &str) -> Result<Option<f64>> {
    if equals_ignoring_case(text.as_bytes(), b"AUTO") {
        return Ok(None);
    }
    let text = text.strip_prefix("1:").unwrap_or(text);
    let Some(value) = parse_finite_double(text) else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "paper style scale takes AUTO or a positive finite denominator N or 1:N",
            text,
        ));
    };
    validate_paper_style_scale(Some(value))?;
    Ok(Some(value))
}

#[derive(Clone, Debug)]
pub struct ViewState {
    pub id: ViewId,
    pub kind: ViewKind,
    /// Shown in the title: the second plan view open is "Plan 2". The lowest
    /// number no other open view of the same kind holds, so closing "Plan 1"
    /// and opening another plan gives "Plan 1" again rather than counting up
    /// for the whole session.
    pub number: i32,

    // Everything below survives a change of kind: switching a view to 3D and
    // back keeps its plan zoom, its hidden layers and its section.
    /// Model3D and Elevation.
    pub camera: Camera,
    /// The kind `camera` was last pointed for by configure_camera. A section
    /// draws through its own transform and leaves the camera alone, so a 3D
    /// view turned into a section and back finds its orbit where it left it;
    /// only arriving at a kind the camera was NOT set up for resets it.
    pub camera_kind: ViewKind,
    /// True once a 3D or elevation view has framed `camera` on something it
    /// draws. The widget is rebuilt on every change of kind, and a new one
    /// frames at its first paint unless this says the camera already has been:
    /// that frame resets the target and the distance, so a trip through a
    /// section kept only the angle of the orbit. set_kind clears it whenever
    /// it points the camera afresh, because a camera pointed afresh has framed
    /// nothing yet.
    pub camera_framed: bool,
    /// Plan: pan and zoom.
    pub plan: ViewTransform,
    pub paper_style_scale: Option<f64>,
    /// False until the plan view has been framed once. The plan widget frames
    /// at its first paint, not its first resize, because a dock's first resize
    /// may be provisional; and until the user pans or zooms it refits that
    /// frame on every resize, so setting this true stops the first frame but
    /// not the refit of a later Zoom Extents or zoom-to.
    pub plan_framed: bool,
    /// Layers this view hides on top of the document's own visibility.
    pub layers: LayerOverrides,
    /// Reference layers (imported rasters and point clouds) this view hides, by
    /// the id the application gave them. cad cannot see interop, where those
    /// layers live, so it keeps the ids without interpreting them; they are
    /// here only so they survive a change of kind with the rest of the view.
    pub hidden_references: BTreeSet<u64>,
    /// Section: what was cut, and how it is drawn.
    pub section: Option<Section>,
    pub section_exaggeration: f64,
    /// Document::surfaces_generation when `section` was cut. A section is a
    /// measured picture of the surfaces that were shown then; the window cuts
    /// it again when the generation moves, and a view whose generation has
    /// moved without being cut again - it could not be, for want of a surface
    /// to cut - says the picture is of surfaces it no longer has.
    pub section_surfaces: u64,
    /// The design profile that was cut beside the ground in `section`, kept
    /// with the section's own alignment as the recipe to cut it again with;
    /// none for a section cut along a line.
    pub section_design: Option<SectionDesign>,

    /// Linked views (view_link.rs): every linked view shows the same centre
    /// and scale, so a design view zoomed takes the as-built view with it.
    /// One link per workspace; plan views only. Like the zoom it keeps in
    /// step, view state: not saved, not undoable, not a document change.
    pub linked: bool,
    /// When the user last panned or zoomed this view, on ViewSet's clock of
    /// such moves (note_moved); 0 when never. It decides which view the others
    /// come to when views are linked (ViewSet::link_leader_for).
    pub last_moved: u64,
    /// Whether a selected entity on a layer this view hides (and the document
    /// shows) is drawn here as a ghost (selection_style.rs): on, so a
    /// selection made in the design view shows in the as-built view that hides
    /// its layer. Its Layers popup and VIEWS SET <id> ghosts=on|off set it.
    pub selection_ghosts: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            id: NO_VIEW,
            kind: ViewKind::Plan,
            number: 1,
            camera: Camera::default(),
            camera_kind: ViewKind::Plan,
            camera_framed: false,
            plan: ViewTransform::default(),
            paper_style_scale: None,
            plan_framed: false,
            layers: LayerOverrides::default(),
            hidden_references: BTreeSet::new(),
            section: None,
            section_exaggeration: 10.0,
            section_surfaces: 0,
            section_design: None,
            linked: false,
            last_moved: 0,
            selection_ghosts: true,
        }
    }
}

// ---- the paper-style scales the project keeps ----------------------------------------------

/// Only the paper-style scale assignments are persisted, not cameras, links or
/// hidden layers. IDs are real plan views when assigned; headless commands may
/// name only an assignment already recorded by the desktop.
pub type ViewPaperScales = BTreeMap<ViewId, Option<f64>>;

pub fn view_paper_scales_to_json(scales: &ViewPaperScales) -> Result<String> {
    let mut records = Vec::with_capacity(scales.len());
    for (&id, &scale) in scales {
        if id == NO_VIEW {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a saved paper-scale view id cannot be zero",
            ));
        }
        validate_paper_style_scale(scale)?;
        records.push(Json::object_of([
            ("view", Json::from(id)),
            ("paper_scale", scale.map_or(Json::Null, Json::from)),
        ]));
    }
    Ok(Json::object_of([
        ("version", Json::from(PAPER_SCALES_VERSION)),
        ("views", Json::Array(records)),
    ])
    .dump())
}

// The C++'s `record.at("view") <= 0 || record.at("view") >
// numeric_limits<ViewId>::max()` on a value already known to be an integer:
// nlohmann compares an integer of either sign with the bound as numbers.
fn is_view_id(value: &Json) -> bool {
    match value {
        Json::NumberInteger(number) => (1..=i64::from(ViewId::MAX)).contains(number),
        Json::NumberUnsigned(number) => (1..=u64::from(ViewId::MAX)).contains(number),
        _ => false,
    }
}

pub fn view_paper_scales_from_json(text: &str) -> Result<ViewPaperScales> {
    let json = match Json::parse(text) {
        Ok(json) => json,
        Err(error) => {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "invalid view paper scales JSON",
                error.what,
            ));
        }
    };
    // find() is None for a value that is not an object, as `is_object() &&
    // contains()` is false for it.
    let Some(version) = json
        .find("version")
        .filter(|version| version.is_number_integer())
    else {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "view paper scales JSON needs an integer version",
        ));
    };
    if *version != Json::from(PAPER_SCALES_VERSION) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "unsupported view paper scales JSON version",
        ));
    }
    let Some(views) = json.find("views").and_then(Json::as_array) else {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "view paper scales JSON needs a views array",
        ));
    };
    let mut scales = ViewPaperScales::new();
    for record in views {
        let Some(id_value) = record
            .find("view")
            .filter(|value| value.is_number_integer() && is_view_id(value))
        else {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "a saved paper-scale view id is a whole number from 1",
            ));
        };
        let id = id_value.get::<ViewId>().unwrap_or_default();
        let mut scale = None;
        if let Some(value) = record.find("paper_scale")
            && !value.is_null()
        {
            if !value.is_number() {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    "paper_scale in view paper scales JSON is a number or null",
                ));
            }
            // get<double>(): an integer is read as the double it names.
            scale = value.get::<f64>().ok();
        }
        validate_paper_style_scale(scale)?;
        match scales.entry(id) {
            Entry::Occupied(_) => {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "duplicate view id in view paper scales JSON",
                    format!("{id}"),
                ));
            }
            Entry::Vacant(slot) => {
                slot.insert(scale);
            }
        }
    }
    Ok(scales)
}

pub fn view_paper_scales(document: &Document) -> Result<ViewPaperScales> {
    match document.metadata().unknown_keys.get(PAPER_SCALES_KEY) {
        None => Ok(ViewPaperScales::new()),
        Some(text) => view_paper_scales_from_json(text),
    }
}

/// One document undo step, preserving unrelated metadata and refusing to
/// overwrite damaged or newer JSON. An unchanged set is not an edit.
///
/// Like the sheet-set command, the step changes only its own metadata key
/// (Document::metadata_key_command: the C++'s ViewPaperScaleAccess and
/// ViewPaperScaleCommand). set_metadata would also mark the metadata
/// independently dirty, leaving an undo back to a saved position incorrectly
/// modified.
pub fn set_view_paper_scales(document: &mut Document, scales: &ViewPaperScales) -> Status {
    let current = view_paper_scales(document)?;
    let encoded = view_paper_scales_to_json(scales)?;
    if current == *scales {
        return Ok(());
    }
    let before = document
        .metadata()
        .unknown_keys
        .get(PAPER_SCALES_KEY)
        .cloned();
    let after = if scales.is_empty() {
        None
    } else {
        Some(encoded)
    };
    let command =
        document.metadata_key_command("SET_VIEW_PAPER_SCALE", PAPER_SCALES_KEY, before, after);
    document.execute(command)
}

/// Re-reads the assignments after open, undo or redo. Only existing views are
/// updated; it never invents a view or moves its camera. Returns the changed
/// IDs, for their widgets to invalidate their kept image.
pub fn restore_view_paper_scales(document: &Document, views: &mut ViewSet) -> Result<Vec<ViewId>> {
    let scales = view_paper_scales(document)?;
    let mut changed = Vec::new();
    for view in views.views_mut() {
        let scale = scales.get(&view.id).copied().flatten();
        if view.paper_style_scale != scale {
            view.paper_style_scale = scale;
            changed.push(view.id);
        }
    }
    Ok(changed)
}

// ---- the set ---------------------------------------------------------------------------------

/// What link() did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkChange {
    /// The view the others came to.
    pub leader: ViewId,
    /// Every member afterwards, in creation order.
    pub linked: Vec<ViewId>,
    /// The members brought to the leader, in creation order: every other
    /// member, or none while the leader has never been framed (a view framed
    /// at its first paint moves the link then, ViewSet::follow).
    pub moved: Vec<ViewId>,
}

#[derive(Clone, Debug)]
pub struct ViewSet {
    // Boxed, as the C++ holds unique_ptrs: take() hands a closing widget the
    // very object the set held, at the address it always had, and
    // ViewSet.TakingAViewHandsOverItsStateAndForgetsIt asserts that identity.
    #[allow(
        clippy::vec_box,
        reason = "take() hands over the same object the set held (the C++ unique_ptr)."
    )]
    views: Vec<Box<ViewState>>,
    /// Every open view, least recently active first.
    recent: Vec<ViewId>,
    active: ViewId,
    next: ViewId,
    /// The clock ViewState::last_moved is read on: one tick per user move.
    moves: u64,
}

impl Default for ViewSet {
    fn default() -> Self {
        Self {
            views: Vec::new(),
            recent: Vec::new(),
            active: NO_VIEW,
            next: 1,
            moves: 0,
        }
    }
}

fn no_such_view(id: ViewId) -> Error {
    make_error(ErrorCode::NotFound, "no such view", format!("{id}"))
}

fn not_open(id: ViewId) -> Error {
    make_error(
        ErrorCode::NotFound,
        format!("no view {id} is open (VIEWS lists them)"),
        format!("{id}"),
    )
}

// `from` and `to` of one slice, the first lent to read and the second to
// change: the C++ follow reads one view while it changes the others.
fn pair_mut(views: &mut [Box<ViewState>], from: usize, to: usize) -> (&ViewState, &mut ViewState) {
    if from < to {
        let (left, right) = views.split_at_mut(to);
        (left[from].as_ref(), right[0].as_mut())
    } else {
        let (left, right) = views.split_at_mut(from);
        (right[0].as_ref(), left[to].as_mut())
    }
}

impl ViewSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new view with the camera its kind starts with. The first view added to
    /// an empty set becomes active; later ones do not, because opening a view
    /// and activating it are separate decisions for the workspace.
    pub fn add(&mut self, kind: ViewKind) -> &mut ViewState {
        let id = self.next;
        // Unsigned, as the C++ counter is: it wraps rather than stops.
        self.next = self.next.wrapping_add(1);
        let mut view = Box::new(ViewState {
            id,
            kind,
            number: self.lowest_free_number(kind, id),
            camera_kind: kind,
            ..ViewState::default()
        });
        configure_camera(&mut view.camera, kind);
        self.views.push(view);
        // Least recent: a view nobody has clicked yet must not outrank one the
        // user was just working in.
        self.recent.insert(0, id);
        if self.active == NO_VIEW {
            self.active = id;
            self.touch(id);
        }
        let last = self.views.len() - 1;
        self.views[last].as_mut()
    }

    /// Recreates a view under a known id - restoring a saved workspace, whose
    /// dock names carry the ids. Fails with InvalidArgument for NO_VIEW and
    /// AlreadyExists when the id is open. Later add()s never reuse it.
    pub fn restore(&mut self, id: ViewId, kind: ViewKind) -> Result<&mut ViewState> {
        if id == NO_VIEW {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a view id cannot be zero",
            ));
        }
        if self.find(id).is_some() {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "a view with this id is already open",
                format!("{id}"),
            ));
        }
        let saved = self.next;
        self.next = id;
        let _ = self.add(kind);
        self.next = saved.max(id.wrapping_add(1));
        let last = self.views.len() - 1;
        Ok(self.views[last].as_mut())
    }

    /// Fails with NotFound for an id that is not open. Removing the active view
    /// activates the most recently active of the rest. A linked view leaves
    /// the link, which is dissolved when one member is left (unlink).
    pub fn remove(&mut self, id: ViewId) -> Status {
        match self.take(id) {
            Some(_) => Ok(()),
            None => Err(no_such_view(id)),
        }
    }

    /// remove(), handing the state to the caller instead of destroying it; None
    /// for an id that is not open. For a widget that is being torn down after
    /// its view has closed and still holds the state: the state must outlive
    /// the widget, and the view must stop being findable at once.
    pub fn take(&mut self, id: ViewId) -> Option<Box<ViewState>> {
        let at = self.views.iter().position(|view| view.id == id)?;
        // Out of the link first, while the view is still one of the set: the
        // members left behind are counted without it.
        let _ = self.leave_link(id);
        let taken = self.views.remove(at);
        self.recent.retain(|&recent| recent != id);
        if self.active == id {
            self.active = self.recent.last().copied().unwrap_or(NO_VIEW);
        }
        Some(taken)
    }

    /// Changes what a view shows, and gives the view the lowest free number in
    /// its new kind. Reconfigures the camera only for a model kind it was not
    /// already set up for (ViewState::camera_kind): a 3D view's orbit is not
    /// reset by choosing 3D again, nor by a trip through Section and back.
    /// Reconfiguring clears camera_framed. A linked view that changes kind
    /// leaves the link, as a closed one does (remove). NotFound for an unknown
    /// id.
    pub fn set_kind(&mut self, id: ViewId, kind: ViewKind) -> Status {
        let Some(view) = self.find(id) else {
            return Err(no_such_view(id));
        };
        if view.kind == kind {
            return Ok(());
        }
        // What it shows now is not what the link keeps in step, and following
        // would move it by numbers that mean something else for its new kind.
        let _ = self.leave_link(id);
        let number = self.lowest_free_number(kind, id);
        let Some(view) = self.find_mut(id) else {
            return Err(no_such_view(id));
        };
        view.kind = kind;
        view.number = number;
        // Section does not look through the camera, so it leaves it; and
        // arriving back at the kind the camera is already set up for leaves it
        // too. The second half is what makes configure_camera's promise - a
        // trip through a section restores the view - true: resetting on every
        // change of kind lost a 3D view's orbit on the way back
        // (ViewSet.AThreeDViewTurnedIntoASectionAndBackKeepsItsOrbit).
        if kind != ViewKind::Section && kind != view.camera_kind {
            configure_camera(&mut view.camera, kind);
            view.camera_kind = kind;
            view.camera_framed = false;
        }
        Ok(())
    }

    /// NotFound, and nothing changes, for an unknown id: activating some other
    /// view than the one asked for is worse than doing nothing.
    pub fn activate(&mut self, id: ViewId) -> Status {
        if self.find(id).is_none() {
            return Err(no_such_view(id));
        }
        self.active = id;
        self.touch(id);
        Ok(())
    }

    pub fn find(&self, id: ViewId) -> Option<&ViewState> {
        self.views
            .iter()
            .map(Box::as_ref)
            .find(|view| view.id == id)
    }

    pub fn find_mut(&mut self, id: ViewId) -> Option<&mut ViewState> {
        self.views
            .iter_mut()
            .map(Box::as_mut)
            .find(|view| view.id == id)
    }

    /// None only when the set is empty.
    pub fn active(&self) -> Option<&ViewState> {
        self.find(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut ViewState> {
        self.find_mut(self.active)
    }

    pub fn active_id(&self) -> ViewId {
        self.active
    }

    // The id most_recent answers with.
    fn most_recent_id(&self, kind: ViewKind) -> Option<ViewId> {
        self.recent
            .iter()
            .rev()
            .copied()
            .find(|&id| self.find(id).is_some_and(|view| view.kind == kind))
    }

    /// The active view when it is of `kind`; otherwise the view of that kind
    /// that was active most recently; otherwise the first of that kind opened;
    /// None when none is open. What Plot, F9 and the Standard Views act on, so
    /// they reach the 3D view the user last touched even after they clicked
    /// back into the plan.
    pub fn most_recent(&self, kind: ViewKind) -> Option<&ViewState> {
        self.most_recent_id(kind).and_then(|id| self.find(id))
    }

    pub fn most_recent_mut(&mut self, kind: ViewKind) -> Option<&mut ViewState> {
        let id = self.most_recent_id(kind)?;
        self.find_mut(id)
    }

    /// Creation order.
    pub fn views(&self) -> Vec<&ViewState> {
        self.views.iter().map(Box::as_ref).collect()
    }

    pub fn views_mut(&mut self) -> Vec<&mut ViewState> {
        self.views.iter_mut().map(Box::as_mut).collect()
    }

    pub fn size(&self) -> usize {
        self.views.len()
    }

    pub fn empty(&self) -> bool {
        self.views.is_empty()
    }

    pub fn count(&self, kind: ViewKind) -> usize {
        self.views.iter().filter(|view| view.kind == kind).count()
    }

    /// "Plan 1", "3D 2", "Section 1", "Elevation 1".
    pub fn title(view: &ViewState) -> String {
        format!("{} {}", view.kind.name(), view.number)
    }

    fn lowest_free_number(&self, kind: ViewKind, except: ViewId) -> i32 {
        let mut number = 1;
        while self
            .views
            .iter()
            .any(|view| view.id != except && view.kind == kind && view.number == number)
        {
            number += 1;
        }
        number
    }

    fn touch(&mut self, id: ViewId) {
        self.recent.retain(|&recent| recent != id);
        self.recent.push(id);
    }

    // ---- linked views (view_link.rs; docs/desktop.md, "Linked views") --------
    //
    // One link: a view is in it or not. Every member shows the centre and
    // scale of the member that moved last, each at its own size and with its
    // own hidden layers. Joining, closing, a change of kind and an unlink are
    // the only ways in and out; none of them moves a view but joining.

    /// The members, in creation order.
    pub fn linked_views(&self) -> Vec<ViewId> {
        self.views
            .iter()
            .filter(|view| view.linked)
            .map(|view| view.id)
            .collect()
    }

    /// Puts `ids` in the link and brings every other member to the leader:
    /// `to` when given, else the link's lowest-id member, else - the link was
    /// empty - the first of `ids`. Duplicate ids count once, and a view
    /// already linked stays. Refused, with nothing changed: NotFound naming an
    /// id that is not open; InvalidArgument naming a view whose kind does not
    /// link (linkable), or when `ids` is empty, or for a `to` that is neither
    /// among `ids` nor already a member. `to` is the C++'s optional argument.
    pub fn link(&mut self, ids: &[ViewId], to: Option<ViewId>) -> Result<LinkChange> {
        // Everything is checked before anything changes, so a refused line
        // leaves the link as it was.
        let mut joining: Vec<ViewId> = Vec::new();
        for &id in ids {
            let Some(view) = self.find(id) else {
                return Err(not_open(id));
            };
            if !linkable(view.kind) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("only plan views link: view {id} is {}", view.kind.name()),
                    format!("{id}"),
                ));
            }
            if !joining.contains(&id) {
                joining.push(id);
            }
        }
        let Some(&first) = joining.first() else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "name the views to link: VIEWS LINK <id>[,<id>...] [TO <id>]",
            ));
        };
        let members = self.linked_views();
        let mut leader = first;
        if let Some(to) = to {
            if self.find(to).is_none() {
                return Err(not_open(to));
            }
            if !joining.contains(&to) && !members.contains(&to) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "TO names the view the others come to, one being linked or already \
                         linked: view {to} is neither"
                    ),
                    format!("{to}"),
                ));
            }
            leader = to;
        } else if let Some(&lowest) = members.iter().min() {
            // The link as it stands leads: joining it must not move the views
            // already in it.
            leader = lowest;
        }
        for &id in &joining {
            if let Some(view) = self.find_mut(id) {
                view.linked = true;
            }
        }
        let moved = self.follow(leader);
        Ok(LinkChange {
            leader,
            moved,
            linked: self.linked_views(),
        })
    }

    // `id` leaves the link, and a link left with one member is dissolved.
    // The views that left, in creation order; empty when `id` was not in it.
    fn leave_link(&mut self, id: ViewId) -> Vec<ViewId> {
        let Some(view) = self.find_mut(id) else {
            return Vec::new();
        };
        if !view.linked {
            return Vec::new();
        }
        view.linked = false;
        let mut left = vec![id];
        // A link of one follows nothing and leads nothing; left standing it
        // would make the next view linked follow a view nobody chose. Only a
        // link this left with one member goes: a link of one made on purpose -
        // the first click of two - waits for the second.
        let members = self.linked_views();
        if let [only] = members[..] {
            if let Some(member) = self.find_mut(only) {
                member.linked = false;
            }
            left.push(only);
        }
        left
    }

    /// Takes `ids` out of the link and returns every view that left, in
    /// creation order: an unlink that leaves one member dissolves the link,
    /// and that member is among them. An id that is not linked is no error and
    /// not returned. NotFound, with nothing changed, for an id that is not
    /// open. Moves nothing.
    pub fn unlink(&mut self, ids: &[ViewId]) -> Result<Vec<ViewId>> {
        for &id in ids {
            if self.find(id).is_none() {
                return Err(not_open(id));
            }
        }
        let mut left: Vec<ViewId> = Vec::new();
        for &id in ids {
            for gone in self.leave_link(id) {
                if !left.contains(&gone) {
                    left.push(gone);
                }
            }
        }
        // In creation order, as every list of views here is.
        Ok(self
            .views
            .iter()
            .map(|view| view.id)
            .filter(|id| left.contains(id))
            .collect())
    }

    /// Every member leaves; returned in creation order.
    pub fn unlink_all(&mut self) -> Vec<ViewId> {
        let left = self.linked_views();
        for &id in &left {
            if let Some(view) = self.find_mut(id) {
                view.linked = false;
            }
        }
        left
    }

    /// The user panned or zoomed `id`: it is now the view moved most recently.
    /// Nothing for an id that is not open.
    pub fn note_moved(&mut self, id: ViewId) {
        if self.find(id).is_none() {
            return;
        }
        self.moves += 1;
        let tick = self.moves;
        if let Some(view) = self.find_mut(id) {
            view.last_moved = tick;
        }
    }

    /// The view the others should come to when `joining` is linked: of
    /// `joining` and the members, the one the user moved most recently; when
    /// none was ever moved, the lowest-id member; with no member, `joining`.
    /// Linking in either order then never throws away the zoom the user has
    /// just made. What the Link button sends as VIEWS LINK ... TO.
    pub fn link_leader_for(&self, joining: ViewId) -> ViewId {
        let members = self.linked_views();
        let mut latest = NO_VIEW;
        let mut latest_move = 0;
        for id in std::iter::once(joining).chain(members.iter().copied()) {
            if let Some(view) = self.find(id)
                && view.last_moved > latest_move
            {
                latest = id;
                latest_move = view.last_moved;
            }
        }
        if latest != NO_VIEW {
            return latest;
        }
        members.iter().copied().min().unwrap_or(joining)
    }

    /// `source` moved: every other member takes its centre and scale
    /// (follow_view) and counts as framed. Returns them in creation order.
    /// Nothing, and an empty list, unless `source` is linked and has been
    /// framed. Star-shaped: a follower is never a source in the same pass.
    pub fn follow(&mut self, source: ViewId) -> Vec<ViewId> {
        let Some(from) = self.views.iter().position(|view| view.id == source) else {
            return Vec::new();
        };
        // An unframed view's centre and scale are the defaults a widget starts
        // with, not a place anybody chose: nothing follows them.
        if !self.views[from].linked || !self.views[from].plan_framed {
            return Vec::new();
        }
        let followers: Vec<usize> = self
            .views
            .iter()
            .enumerate()
            .filter(|(at, view)| *at != from && view.linked)
            .map(|(at, _)| at)
            .collect();
        let mut moved = Vec::new();
        for at in followers {
            let (leader, view) = pair_mut(&mut self.views, from, at);
            if follow_view(leader, view) {
                // Framed: the view's widget must not frame it again at its
                // first paint, which would move it off the link.
                view.plan_framed = true;
                moved.push(view.id);
            }
        }
        moved
    }
}
