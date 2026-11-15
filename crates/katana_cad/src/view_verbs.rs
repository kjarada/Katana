//! VIEWS and ZOOM: the window's views on the command line (the owner's request
//! of 2026-09-30 for views that pan and zoom together, with their controls on
//! each view's own bar; docs/cad.md, "The window's views: VIEWS and ZOOM";
//! include/katana/cad/view_verbs.hpp, src/katana_cad/view_verbs.cpp).
//!
//! A control on a view's bar does not move the view itself: it builds one of
//! these lines and hands it to the window's command runner, so a click, a line
//! typed, a script and an agent driving `katana --command` reach the views by
//! ONE path, logged as a typed line is. The Link button runs VIEWS LINK and
//! VIEWS UNLINK.
//!
//! The Document holds no views - the window's workspace does, in a ViewSet -
//! so the verbs reach them through a ViewVerbHost the front end supplies, as
//! the scope word VIEW reaches its view through a ScopeViewProvider.
//! katana_cli and katana_mcp have no live views, and there those operations
//! are refused by name, never as unknown commands. VIEWS SCALE alone can read
//! or change the saved scale assignments, naming a real view recorded by the
//! desktop.
//!
//! ```text
//! VIEWS [LIST]                          a record per open view, in the order
//!                                       they were opened; then views=N linked=M
//! VIEWS OPEN plan|3d|section|elevation  opens one and makes it active
//! VIEWS ACTIVATE <id>
//! VIEWS LINK <id>[,<id>...] [TO <id>]   leader=1 linked=1,2 moved=2
//! VIEWS UNLINK <id>[,<id>...] | ALL     unlinked=1,2 linked=none
//! VIEWS HIDE <id> <layer>[,<layer>...]  the layers that view hides of its own
//! VIEWS SHOW <id> <layer>[,<layer>...] | ALL
//! VIEWS ISOLATE <id> <layer>            that layer, its parents and what lies
//!                                       beneath it, and nothing else
//! VIEWS HIDE|SHOW|ISOLATE <id> LAYER <name>...
//!                                       each word after LAYER is ONE layer's
//!                                       name, commas and all
//! VIEWS HIDE|SHOW <id> REFERENCE <ref>[,<ref>...]
//!                                       the reference layers (an imported
//!                                       raster or point cloud, by the id the
//!                                       Reference Data panel gives it) that
//!                                       view hides of its own
//! VIEWS SET <id> ghosts=on|off          whether that view shows the selection
//!                                       faintly where it hides the layer
//! VIEWS SCALE <id> [AUTO|N|1:N]         query or set its paper-style scale,
//!                                       one persisted, undoable edit
//! VIEWS SCALE LIST                      plan views and their paper-style scales;
//!                                       headless: the saved assignments only
//! ZOOM | Z | 'ZOOM | 'Z                 the active view's extents, as always
//! ZOOM EXTENTS | E | ALL | A | IN [f] | OUT [f] | <f> | <f>X | WINDOW x0,y0,x1,y1
//!      | CENTRE x,y [SCALE s]  [view=<id>]
//! ZOOM SELECTION | DRAWING | VIEW [<id>] [EXTENTS] | AREA x0,y0,x1,y1
//!      | LAYERS a,b [ONLY]  [WHERE key=value ...]  [view=<id>]
//! ```
//!
//! An id is the window's ViewId - what a record's view= says - and never the
//! number in a view's title ("Plan 2" is numbered among the plan views, its id
//! among every view opened). view= may stand anywhere on a ZOOM line and is
//! taken off before anything else is read, so a WHERE never reads it as a
//! condition; without it ZOOM acts on the active view. Linked views follow
//! every ZOOM.
//!
//! EXTENTS frames what the view draws, in any kind of view; ALL and A alone
//! are the same, every CAD program's Zoom All (with a filter after it, ALL is
//! the scope word for the whole drawing). IN and OUT (by 2 unless a factor is
//! given) and a bare factor zoom about the middle of any view - a plan view
//! and a section about their centres, a 3D or elevation view as its wheel
//! zooms, turned that many notches over its middle pixel - and a factor may
//! carry AutoCAD's X, "2X", relative to the view as a bare one always is;
//! WINDOW frames a box as Zoom Extents frames the drawing - a box of the
//! drawing, where the scope word AREA would be the entities found in one;
//! CENTRE puts a point in the middle, at SCALE pixels per unit when given.
//! WINDOW and CENTRE act on plan views. 'ZOOM and 'Z, AutoCAD's transparent
//! form, are ZOOM whether or not a tool runs. A view that has not framed
//! anything yet frames what it draws first, as its first paint would, so a
//! relative zoom never starts from the place a new view holds until then.
//!
//! The scope words are the shared grammar (scope_verbs.rs), read by the one
//! parser and resolved by match_scope. ZOOM alone is still EXTENTS, where no
//! scope word elsewhere is the selection. The reply begins with the scope
//! record; a scope that takes nothing moves no view and says so (matched=0),
//! which is not a refusal. Every kind of view frames what it SHOWS of what the
//! scope took, by one rule: the entities it draws - by its own layers as well
//! as the drawing's - and, with its ghosts on, the selected ones it ghosts (a
//! plan view ghosts no label). So a view never frames empty ground: a
//! selection on a layer it hides with its ghosts off, or on one the drawing
//! hides, is shown nowhere there, and the view does not move - its record
//! says so after the scope's, shown=0 moved=no. A section frames no scope.
//!
//! HIDE, SHOW and ISOLATE are the view's own layer filter, its Layers button's
//! (LayerOverrides): subtractive, so a view never shows what the document
//! hides; unlike its paper-style scale, not saved and not undoable. A layer is
//! a layer of the drawing or a node of the layer tree above one ("design" when
//! only "design/road" is a layer); one the drawing lacks is refused, naming
//! it, and nothing changes. SHOW of a layer beneath one the view hides is
//! refused, naming what holds it: it would stay hidden. A comma separates two
//! layers, so a layer whose name holds one is named with LAYER, which takes
//! every word after it whole. SHOW ALL shows the reference layers as well as
//! the layers. A reference layer the window does not have is refused, naming
//! it (cad cannot see the Reference Data panel's layers; the host answers).
//! The reply is the view's record, whose hiddenrefs= lists the reference
//! layers a view hides.
//!
//! Records, one per line, key=value, reals as format_exact_real writes them
//! and angles in degrees; ghosts= is the view's switch (VIEWS SET), and
//! hidden= the layers a view hides of its own, left out when it hides none:
//!
//! ```text
//! view=1 kind=plan title="Plan 1" active=yes linked=yes centre=50,40 scale=8 area=x0,y0,x1,y1
//!        ghosts=on hidden=asbuilt
//! view=3 kind=3d title="3D 1" active=no target=x,y,z distance=d azimuth=a elevation=e
//!        projection=perspective ghosts=on
//! view=4 kind=section title="Section 1" active=no ghosts=on
//! ```
//!
//! framed=no follows the place of a plan, 3D or elevation view that has not
//! framed anything yet: it frames what it draws at its first paint, and until
//! then its centre and scale, or its target and distance, are the place every
//! new view starts at, which it will not show. Left out once the view has
//! framed.
//!
//! ZOOM replies the record of the view it moved without title, active,
//! linked, ghosts and hidden - where it looks, not what it shows - then a line
//! for each view that followed it, and with scope words the scope's record
//! first, and shown= on the record of the view framed:
//!
//! ```text
//! view=2 kind=plan followed=1 centre=50,40 scale=8 area=x0,y0,x1,y1
//! scope=selection matched=1
//! view=2 kind=plan centre=25,20 scale=... area=... shown=1
//! scope=selection matched=1
//! view=2 kind=plan shown=0 moved=no
//! ```
//!
//! # The Rust shape of ViewVerbHost (decided by the port, wave H6)
//!
//! The C++ host is a virtual class the window's workspace implements, asked
//! for at each line through a `std::function<ViewVerbHost*()>`, and the C++
//! scope provider of the same window reads the very ViewSet the host hands
//! out (`scopeViewOf(views_, id)`) - during ZOOM's scope match, while the verb
//! holds the host. A host lent `&mut` could not be read by that provider at
//! the same time, so here every method takes `&self` and the host keeps its
//! ViewSet in a RefCell it lends through `views()`: the provider borrows the
//! set for its answer, and the verbs borrow it only between calls to the host
//! and the provider, never across one, so neither finds it borrowed. A host
//! keeps whatever else it records (the requests, the changes) in cells of its
//! own. ViewHostProvider is the host borrowed for the line, None being no
//! window (the C++ provider unset or answering null). Rejected: `&mut dyn
//! ViewVerbHost`, which left a window's or a test's scope provider no way to
//! read the views the host holds while ZOOM VIEW ran.

use std::cell::RefCell;

use katana_core::error::make_error;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, parse_finite_double, parse_integer,
};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::layer_path::{is_layer_under, layer_ancestors};
use katana_entity::{EntityId, EnumName, Geometry};
use katana_geometry::{Box2, Point2};
use katana_math::{RAD_TO_DEG, cpp};
use katana_render::Projection;

use crate::document::Document;
use crate::scope_verbs::{
    ScopeMatch, ScopeViewProvider, is_scope_word, match_scope, parse_scope_words, record_value,
    scope_record,
};
use crate::selection::{extent_of, is_drawn, is_ghost};
use crate::view_set::{
    NO_VIEW, ViewId, ViewSet, ViewState, parse_paper_style_scale, restore_view_paper_scales,
    set_view_paper_scales, view_paper_scales,
};
use crate::view_transform::ViewTransform;
use crate::viewport_layout::ViewKind;

const VIEWS_USAGE: &str = "VIEWS [LIST] | OPEN plan|3d|section|elevation | ACTIVATE id | LINK \
     id[,id...] [TO id] | UNLINK id[,id...]|ALL | HIDE id \
     layer[,layer...]|LAYER name...|REFERENCE ref[,ref...] | SHOW id \
     layer[,layer...]|LAYER name...|REFERENCE ref[,ref...]|ALL | ISOLATE id layer|LAYER name | SET \
     id ghosts=on|off | SCALE id [AUTO|N|1:N] | SCALE LIST";
const ZOOM_USAGE: &str = "ZOOM [EXTENTS | IN [f] | OUT [f] | factor | WINDOW x0,y0,x1,y1 | CENTRE \
     x,y [SCALE s] | SELECTION | DRAWING | VIEW [id] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b \
     [ONLY] [WHERE k=v ...]] [view=id]";

/// What ZOOM asks of one view (the C++ `ZoomRequest::Kind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ZoomRequestKind {
    /// What the view draws: its own Zoom Extents.
    #[default]
    Extents,
    /// `factor` times closer, about the view's centre.
    In,
    /// `factor` times farther, about the view's centre.
    Out,
    /// Times `factor` about the view's centre: above 1 is closer.
    Factor,
    /// Frames `window` as Zoom Extents frames the drawing.
    Window,
    /// `centre` in the middle, at `scale` pixels per unit if given.
    Centre,
    /// What a scope took that the view shows: `window`, their extent
    /// (extent_of), framed as WINDOW frames a box in a plan view; `ids`
    /// themselves in a 3D or elevation view, which frames the box its scene
    /// puts them in.
    Scope,
}

/// What ZOOM asks of one view.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoomRequest {
    pub view: ViewId,
    pub kind: ZoomRequestKind,
    /// In, Out and Factor: finite and above 0.
    pub factor: f64,
    pub window: Box2,
    /// Scope: what it took that the view shows (drawn, or ghosted), in
    /// ascending order; `window` is their extent.
    pub ids: Vec<EntityId>,
    pub centre: Point2,
    /// Within ViewTransform's limits: SCALE outside them is refused, not
    /// clamped, because a scale the view cannot show is not the one asked for.
    pub scale: Option<f64>,
}

impl Default for ZoomRequest {
    fn default() -> Self {
        Self {
            view: NO_VIEW,
            kind: ZoomRequestKind::Extents,
            factor: 2.0,
            window: Box2::default(),
            ids: Vec::new(),
            centre: Point2::default(),
            scale: None,
        }
    }
}

/// Whether a view of `kind` takes `request`: EXTENTS, IN, OUT and a factor any
/// view - about the middle of it, a 3D or elevation view by its wheel's own
/// zoom there; a scope a plan, 3D or elevation view, which frame the entities
/// it took (a section shows where they cross it, which has no box to frame);
/// WINDOW and CENTRE a plan view, whose plan position they are. The one rule:
/// ZOOM refuses by it, and a view's bar, the View menu and the Zoom To dialog
/// offer only what it allows, so widening it widens all four.
pub fn zoom_takes(kind: ViewKind, request: ZoomRequestKind) -> bool {
    match request {
        ZoomRequestKind::Extents
        | ZoomRequestKind::In
        | ZoomRequestKind::Out
        | ZoomRequestKind::Factor => true,
        ZoomRequestKind::Scope => kind != ViewKind::Section,
        ZoomRequestKind::Window | ZoomRequestKind::Centre => kind == ViewKind::Plan,
    }
}

/// In, Out, Factor and Centre applied to a plan view's transform: arithmetic
/// about the view's centre, which needs neither the widget nor its size. The
/// scale is clamped to ViewTransform's limits as the wheel's is. Window and
/// Extents are the widget's - they fit what it draws, at its size, with its
/// margin - and return false here, the transform untouched.
pub fn apply_plan_zoom(view: &mut ViewTransform, request: &ZoomRequest) -> bool {
    // About the centre pixel, where screen_to_world gives the centre back
    // exactly: ViewTransform::zoom_at then leaves the centre where it was and
    // clamps the scale as the wheel's zoom does.
    let middle = Point2::new(0.5 * view.width_pixels, 0.5 * view.height_pixels);
    match request.kind {
        ZoomRequestKind::In | ZoomRequestKind::Factor => {
            view.zoom_at(middle, request.factor);
            true
        }
        ZoomRequestKind::Out => {
            view.zoom_at(middle, 1.0 / request.factor);
            true
        }
        ZoomRequestKind::Centre => {
            view.center = request.centre;
            if let Some(scale) = request.scale {
                view.scale = scale;
            }
            true
        }
        ZoomRequestKind::Extents | ZoomRequestKind::Window | ZoomRequestKind::Scope => false,
    }
}

/// The window's side: what the verbs need of the views the Document does not
/// hold. The window's workspace answers it; a test answers with a ViewSet of
/// its own. Every method takes `&self` (see the module comment): a host keeps
/// what it changes in cells.
pub trait ViewVerbHost {
    /// The open views: the verbs read them, and link and unlink them, here.
    /// Borrowed only between the other calls, never across one.
    fn views(&self) -> &RefCell<ViewSet>;
    /// Opens a view of `kind` and makes it the active one: its id.
    fn open(&self, kind: ViewKind) -> Result<ViewId>;
    /// NotFound for an id that is not open.
    fn activate(&self, id: ViewId) -> Status;
    /// Moves `request.view` as asked, through its widget, and then every view
    /// linked with it: the view moved, then each view that followed it, in
    /// the order they were opened - or none, when a 3D view draws nothing of
    /// what a scope took and so has nothing to frame. The request has been
    /// checked against the view's kind; a host fails only with what its
    /// widget refuses.
    fn zoom(&self, request: &ZoomRequest) -> Result<Vec<ViewId>>;
    /// The verbs changed the link: these views joined or left it, and may have
    /// been moved behind their widgets' backs. Repaint them, keep a linked view
    /// from refitting its old frame on a resize, and bring every view's bar up
    /// to date.
    fn changed(&self, ids: &[ViewId]);
    /// The verbs changed what `id` draws of its own - the layers it hides
    /// (ViewState::layers) or its ghosts of the selection (selection_ghosts):
    /// no document change, so nothing else hears of it. Redraw that view and
    /// bring its Layers button up to date.
    fn settings_changed(&self, id: ViewId);
    /// Whether `id` is a reference layer the window has - an imported raster
    /// or point cloud, by the id of the Reference Data panel - so that VIEWS
    /// HIDE and SHOW can refuse one it has not. cad holds the ids a view hides
    /// (ViewState::hidden_references) and cannot read the data; a host with
    /// none answers no, the default.
    fn has_reference(&self, _id: u64) -> bool {
        false
    }
}

/// The host lent for one VIEWS or ZOOM line; None is no window (the C++
/// provider unset, or answering null): only VIEWS SCALE's saved assignments
/// remain available. The C++ asks its provider at each line so a front end can
/// hand over a host that did not exist when the interpreter was made; here the
/// caller asks whatever it keeps and lends the answer.
pub type ViewHostProvider<'a> = Option<&'a dyn ViewVerbHost>;

/// VIEWS or ZOOM, after the interpreter's aliases (Z is ZOOM).
pub fn is_view_verb(verb: &str) -> bool {
    verb == "VIEWS" || verb == "ZOOM"
}

/// Runs one VIEWS or ZOOM line: `verb` as is_view_verb takes it, `args` the
/// words after it as the interpreter splits them. `scope_views` answers the
/// scope word VIEW, as it does for every verb that takes the shared scope (the
/// C++ default is none). The reply is the records above; refusals name the
/// word, the view or the kind at fault.
pub fn run_view_verb(
    document: &mut Document,
    verb: &str,
    args: &[String],
    host: ViewHostProvider<'_>,
    scope_views: ScopeViewProvider<'_>,
) -> Result<String> {
    if let Some(host) = host {
        let restored = restore_view_paper_scales(document, &mut host.views().borrow_mut());
        if let Ok(restored) = restored {
            for id in restored {
                host.settings_changed(id);
            }
        }
    }
    if verb == "VIEWS" && args.first().is_some_and(|word| upper(word) == "SCALE") {
        return paper_scale_verb(document, host, args);
    }
    let Some(host) = host else {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "{verb} is the desktop window's: it acts on the window's views, which katana_cli \
                 and katana_mcp do not have; run it on the window's command line or with katana \
                 --command"
            ),
        ));
    };
    if verb == "VIEWS" {
        return views_verb(document, host, args);
    }
    zoom_verb(document, host, args, scope_views)
}

/// The record VIEWS gives for `view`, a view of `views`.
pub fn view_record(views: &ViewSet, view: &ViewState) -> String {
    let paper = match view.paper_style_scale {
        Some(scale) if view.kind == ViewKind::Plan => format!(" paper_scale={}", real(scale)),
        _ => String::new(),
    };
    format!(
        "view={} kind={} title={} active={}{}{paper} ghosts={}{}",
        view.id,
        kind_word(view.kind),
        record_value(&ViewSet::title(view)),
        if views.active_id() == view.id {
            "yes"
        } else {
            "no"
        },
        place_of(view, true),
        if view.selection_ghosts { "on" } else { "off" },
        hidden_of(view)
    )
}

// ---- words ---------------------------------------------------------------------------------

// The C++ `core::uppered` of a word, as text: ASCII letters only, so a word
// that is UTF-8 stays UTF-8.
fn upper(word: &str) -> String {
    word.to_ascii_uppercase()
}

// The word a record and VIEWS OPEN use for a kind: the title's word, lower
// case, so "3D 1" is kind=3d.
fn kind_word(kind: ViewKind) -> String {
    kind.name().to_ascii_lowercase()
}

fn kind_from(word: &str) -> Option<ViewKind> {
    [
        ViewKind::Plan,
        ViewKind::Model3D,
        ViewKind::Section,
        ViewKind::Elevation,
    ]
    .into_iter()
    .find(|kind| equals_ignoring_case(word.as_bytes(), kind.name().as_bytes()))
}

fn real(value: f64) -> String {
    format_exact_real(value)
}

fn point_text(point: Point2) -> String {
    format!("{},{}", real(point.x), real(point.y))
}

fn listed(ids: &[ViewId]) -> String {
    if ids.is_empty() {
        return String::from("none");
    }
    let words: Vec<String> = ids.iter().map(|id| format!("{id}")).collect();
    words.join(",")
}

fn not_open(id: ViewId) -> Error {
    make_error(
        ErrorCode::NotFound,
        format!("no view {id} is open (VIEWS lists them)"),
        format!("{id}"),
    )
}

// A view id as typed: a whole number from 1.
fn parse_view_id(word: &str) -> Result<ViewId> {
    let digits = !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit());
    let value = if digits { parse_integer(word) } else { None };
    value
        .filter(|value| (1..=i64::from(ViewId::MAX)).contains(value))
        .and_then(|value| ViewId::try_from(value).ok())
        .ok_or_else(|| {
            make_error(
                ErrorCode::ParseFailure,
                "a view id is a whole number from 1: the view= a record gives (VIEWS lists them), \
                 not the number in a view's title",
                word,
            )
        })
}

// "1,2" and "1 2", up to TO or the end of the line; each at most once.
fn parse_ids(args: &[String], at: &mut usize) -> Result<Vec<ViewId>> {
    let mut ids = Vec::new();
    while *at < args.len() && upper(&args[*at]) != "TO" {
        // "1,,2" and a trailing comma are stray commas, not ids.
        for part in args[*at].split(',').filter(|part| !part.is_empty()) {
            let id = parse_view_id(part)?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        *at += 1;
    }
    Ok(ids)
}

// The layers of HIDE and SHOW: comma lists in one word or several, each once,
// each a layer of the drawing or a node of the layer tree above one - what
// the Layers popup lists. A name with a blank in it is quoted, as the
// interpreter's words are.
//
// LAYER, first, and at least one word after it, turns the commas off: every
// word after it is ONE layer's whole name. A comma is a legal character in a
// layer's name, and a comma list cannot say such a name; this can, and the
// Layers popup writes it for a name that is not plain. A line whose only word
// is LAYER still names the layer called LAYER.
fn parse_layers(document: &Document, args: &[String], from: usize) -> Result<Vec<String>> {
    let mut layers: Vec<String> = Vec::new();
    if from + 1 < args.len() && upper(&args[from]) == "LAYER" {
        for word in &args[from + 1..] {
            if !word.is_empty() && !layers.contains(word) {
                layers.push(word.clone());
            }
        }
    } else {
        for word in args.iter().skip(from) {
            for part in word.split(',').filter(|part| !part.is_empty()) {
                let layer = String::from(part);
                if !layers.contains(&layer) {
                    layers.push(layer);
                }
            }
        }
    }
    let names = document.model().layers.names();
    for layer in &layers {
        let exists = names.iter().any(|name| is_layer_under(name, layer));
        if !exists {
            return Err(make_error(
                ErrorCode::NotFound,
                format!("no layer '{layer}' in the drawing (LAYER LIST lists them)"),
                layer.as_str(),
            ));
        }
    }
    Ok(layers)
}

// The reference layers (REFERENCE ids, as the Reference Data panel and RASTER
// and CLOUD words know them) of HIDE and SHOW: comma lists in one word or
// several, each id once, each one the window has. cad cannot see Reference
// Data, so the host says whether an id is one.
fn parse_references(host: &dyn ViewVerbHost, args: &[String], from: usize) -> Result<Vec<u64>> {
    let mut ids = Vec::new();
    for word in args.iter().skip(from) {
        for text in word.split(',').filter(|text| !text.is_empty()) {
            let digits = text.bytes().all(|byte| byte.is_ascii_digit());
            let value = if digits { parse_integer(text) } else { None };
            let Some(id) = value
                .filter(|&value| value >= 1)
                .and_then(|value| u64::try_from(value).ok())
            else {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!(
                        "a reference layer is named by its id, a whole number from 1 (the \
                         Reference Data panel lists them), not '{text}'"
                    ),
                    text,
                ));
            };
            if !host.has_reference(id) {
                return Err(make_error(
                    ErrorCode::NotFound,
                    format!(
                        "no reference layer {id} in the drawing (the Reference Data panel lists \
                         them)"
                    ),
                    text,
                ));
            }
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    Ok(ids)
}

// The layers a view hides of its own, for its record: none, or hidden=a,b; and
// the reference layers it hides, hiddenrefs=7,9, left out when it hides none.
fn hidden_of(view: &ViewState) -> String {
    let mut text = String::new();
    if !view.layers.empty() {
        let layers: Vec<&str> = view.layers.hidden().iter().map(String::as_str).collect();
        text = format!(" hidden={}", record_value(&layers.join(",")));
    }
    if !view.hidden_references.is_empty() {
        let references: Vec<String> = view
            .hidden_references
            .iter()
            .map(|id| format!("{id}"))
            .collect();
        text += &format!(" hiddenrefs={}", references.join(","));
    }
    text
}

// Where a view is looking, after its id, kind and (for VIEWS) title and
// active: a plan view's centre, scale and area; a 3D or elevation view's
// camera; nothing for a section, whose pan and zoom its widget keeps. Then
// framed=no while the view has framed nothing: the place is the one every
// new view starts at, and the view's first paint will leave it - an agent
// reading VIEWS OPEN's record took it for where the view looks.
fn place_of(view: &ViewState, with_linked: bool) -> String {
    let mut text = String::new();
    let mut framed = true;
    match view.kind {
        ViewKind::Plan => {
            if with_linked {
                text += if view.linked {
                    " linked=yes"
                } else {
                    " linked=no"
                };
            }
            let area = view.plan.visible_world_bounds();
            text += &format!(
                " centre={} scale={} area={},{}",
                point_text(view.plan.center),
                real(view.plan.scale),
                point_text(area.min),
                point_text(area.max)
            );
            framed = view.plan_framed;
        }
        ViewKind::Model3D | ViewKind::Elevation => {
            let camera = &view.camera;
            let target = camera.target();
            text += &format!(
                " target={},{},{} distance={} azimuth={} elevation={} projection={}",
                real(target.x),
                real(target.y),
                real(target.z),
                real(camera.distance()),
                real(camera.azimuth() * RAD_TO_DEG),
                real(camera.elevation() * RAD_TO_DEG),
                if camera.projection() == Projection::Perspective {
                    "perspective"
                } else {
                    "orthographic"
                }
            );
            framed = view.camera_framed;
        }
        ViewKind::Section => {}
    }
    if framed { text } else { text + " framed=no" }
}

fn paper_scale_record(id: ViewId, scale: Option<f64>) -> String {
    format!(
        "view={id} kind=plan paper_scale={}",
        scale.map_or_else(|| String::from("auto"), real)
    )
}

fn paper_scale_verb(
    document: &mut Document,
    host: ViewHostProvider<'_>,
    args: &[String],
) -> Result<String> {
    const USAGE: &str =
        "VIEWS SCALE takes a view id and optionally AUTO, N or 1:N; VIEWS SCALE LIST lists them";
    if !(2..=3).contains(&args.len()) {
        return Err(Error::new(ErrorCode::ParseFailure, USAGE));
    }
    let mut scales = view_paper_scales(document)?;
    if upper(&args[1]) == "LIST" {
        if args.len() != 2 {
            return Err(Error::new(ErrorCode::ParseFailure, USAGE));
        }
        let mut reply = String::new();
        let mut count = 0_usize;
        if let Some(host) = host {
            for view in host.views().borrow().views() {
                if view.kind == ViewKind::Plan {
                    reply += &paper_scale_record(view.id, view.paper_style_scale);
                    reply.push('\n');
                    count += 1;
                }
            }
        } else {
            for (&id, &scale) in &scales {
                reply += &paper_scale_record(id, scale);
                reply.push('\n');
                count += 1;
            }
        }
        return Ok(format!("{reply}views={count}"));
    }
    let id = parse_view_id(&args[1])?;
    let before = if let Some(host) = host {
        let views = host.views().borrow();
        let Some(view) = views.find(id) else {
            return Err(not_open(id));
        };
        if view.kind != ViewKind::Plan {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "paper style scale applies to a plan view, not {}",
                    kind_word(view.kind)
                ),
                format!("{id}"),
            ));
        }
        view.paper_style_scale
    } else {
        let Some(&found) = scales.get(&id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                format!(
                    "no saved plan view {id} has a paper style scale; VIEWS SCALE LIST lists \
                     saved views. Set the scale on a real desktop plan view and save the project \
                     first"
                ),
                format!("{id}"),
            ));
        };
        found
    };
    if args.len() == 2 {
        return Ok(paper_scale_record(id, before));
    }
    let scale = parse_paper_style_scale(&args[2])?;
    let changed = before != scale;
    if changed {
        scales.insert(id, scale);
        set_view_paper_scales(document, &scales)?;
        if let Some(host) = host {
            let restored = restore_view_paper_scales(document, &mut host.views().borrow_mut())?;
            for restored_id in restored {
                host.settings_changed(restored_id);
            }
        }
    }
    Ok(format!(
        "{} changed={}",
        paper_scale_record(id, scale),
        if changed { "yes" } else { "no" }
    ))
}

// ---- VIEWS ---------------------------------------------------------------------------------

fn refuse_views_word(word: &str) -> Error {
    make_error(
        ErrorCode::ParseFailure,
        format!("VIEWS does not take '{word}': {VIEWS_USAGE}"),
        word,
    )
}

// The record of `id` after a verb changed it: the C++ reads the view it holds,
// which a host cannot have closed in between; here the set is asked again.
fn record_of(host: &dyn ViewVerbHost, id: ViewId) -> Result<String> {
    let views = host.views().borrow();
    match views.find(id) {
        Some(view) => Ok(view_record(&views, view)),
        None => Err(not_open(id)),
    }
}

fn layer_shape(action: &str) -> &'static str {
    if action == "ISOLATE" {
        " <id> <layer>"
    } else {
        " <id> <layer>[,<layer>...]"
    }
}

fn views_verb(document: &Document, host: &dyn ViewVerbHost, args: &[String]) -> Result<String> {
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| word.to_ascii_uppercase());

    if action == "LIST" {
        if let Some(word) = args.get(1) {
            return Err(refuse_views_word(word));
        }
        let views = host.views().borrow();
        let mut reply = String::new();
        for view in views.views() {
            reply += &view_record(&views, view);
            reply.push('\n');
        }
        return Ok(format!(
            "{reply}views={} linked={}",
            views.size(),
            views.linked_views().len()
        ));
    }
    if action == "OPEN" {
        if args.len() != 2 {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "VIEWS OPEN takes what the view shows: plan, 3d, section or elevation",
            ));
        }
        let Some(kind) = kind_from(&args[1]) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!(
                    "VIEWS OPEN takes plan, 3d, section or elevation, not '{}'",
                    args[1]
                ),
                args[1].as_str(),
            ));
        };
        let opened = host.open(kind)?;
        let views = host.views().borrow();
        let Some(view) = views.find(opened) else {
            return Err(make_error(
                ErrorCode::Internal,
                "the window opened a view it does not list",
                format!("{opened}"),
            ));
        };
        return Ok(view_record(&views, view));
    }
    if action == "ACTIVATE" {
        if args.len() != 2 {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "VIEWS ACTIVATE takes one view id",
            ));
        }
        let id = parse_view_id(&args[1])?;
        if host.views().borrow().find(id).is_none() {
            return Err(not_open(id));
        }
        host.activate(id)?;
        return record_of(host, id);
    }
    if action == "LINK" {
        let mut at = 1;
        let ids = parse_ids(args, &mut at)?;
        let mut to = None;
        if at < args.len() {
            // parse_ids stopped at TO.
            if at + 2 != args.len() {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "TO takes the one view the others come to: VIEWS LINK <id>[,<id>...] [TO \
                     <id>]",
                    args[at].as_str(),
                ));
            }
            to = Some(parse_view_id(&args[at + 1])?);
        }
        let change = host.views().borrow_mut().link(&ids, to)?;
        host.changed(&change.linked);
        return Ok(format!(
            "leader={} linked={} moved={}",
            change.leader,
            listed(&change.linked),
            listed(&change.moved)
        ));
    }
    if action == "UNLINK" {
        let left = if args.len() == 2 && upper(&args[1]) == "ALL" {
            host.views().borrow_mut().unlink_all()
        } else {
            let mut at = 1;
            let ids = parse_ids(args, &mut at)?;
            if let Some(word) = args.get(at) {
                return Err(refuse_views_word(word));
            }
            if ids.is_empty() {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    "name the views to unlink: VIEWS UNLINK <id>[,<id>...] | ALL",
                ));
            }
            host.views().borrow_mut().unlink(&ids)?
        };
        host.changed(&left);
        let linked = host.views().borrow().linked_views();
        return Ok(format!(
            "unlinked={} linked={}",
            listed(&left),
            listed(&linked)
        ));
    }
    if action == "HIDE" || action == "SHOW" || action == "ISOLATE" {
        // The view's own filter: what its Layers button sets, by a line an
        // agent can type and a test can run - a design view hiding the
        // as-built layers beside an as-built view hiding the design ones.
        if args.len() < 3 {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                format!(
                    "VIEWS {action} takes a view id and the layers: VIEWS {action}{}",
                    layer_shape(&action)
                ),
            ));
        }
        let id = parse_view_id(&args[1])?;
        if host.views().borrow().find(id).is_none() {
            return Err(not_open(id));
        }
        let third = upper(&args[2]);
        if action == "SHOW" && args.len() == 3 && third == "ALL" {
            // Everything the document shows: its layers and its reference layers.
            if let Some(view) = host.views().borrow_mut().find_mut(id) {
                view.layers.clear();
                view.hidden_references.clear();
            }
        } else if args.len() > 3 && third == "REFERENCE" {
            if action == "ISOLATE" {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "VIEWS ISOLATE takes one layer, not a reference layer: VIEWS ISOLATE <id> \
                     <layer>",
                    args[2].as_str(),
                ));
            }
            // Asked of the host before the view is borrowed: the host may
            // read the views to answer.
            let references = parse_references(host, args, 3)?;
            if references.is_empty() {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!(
                        "VIEWS {action} REFERENCE names no reference layer in '{}': VIEWS \
                         {action} <id> REFERENCE <ref>[,<ref>...]",
                        args[3]
                    ),
                    args[3].as_str(),
                ));
            }
            if let Some(view) = host.views().borrow_mut().find_mut(id) {
                for reference in references {
                    if action == "HIDE" {
                        view.hidden_references.insert(reference);
                    } else {
                        view.hidden_references.remove(&reference);
                    }
                }
            }
        } else {
            let layers = parse_layers(document, args, 2)?;
            // Commas alone ("VIEWS HIDE 1 ,") name no layer: refused, where
            // the line changed nothing and replied as though it had worked.
            if layers.is_empty() {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!(
                        "VIEWS {action} names no layer in '{}': VIEWS {action}{}",
                        args[2],
                        layer_shape(&action)
                    ),
                    args[2].as_str(),
                ));
            }
            let mut views = host.views().borrow_mut();
            let Some(view) = views.find_mut(id) else {
                return Err(not_open(id));
            };
            if action == "ISOLATE" {
                if layers.len() != 1 {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "VIEWS ISOLATE takes one layer: the one the view is to show",
                        args[2].as_str(),
                    ));
                }
                // parse_layers found it, so isolate has a branch to keep.
                view.layers
                    .isolate(&layers[0], &document.model().layers.names())?;
            } else {
                // A layer beneath one this view hides stays hidden whatever
                // is shown of it (LayerOverrides::show), so SHOW of it would
                // change nothing and say nothing: refused, naming what holds
                // it, before any layer of the line is shown.
                if action == "SHOW" {
                    for layer in &layers {
                        let held = layer_ancestors(layer).into_iter().find(|ancestor| {
                            view.layers.hides_directly(ancestor) && !layers.contains(ancestor)
                        });
                        if let Some(held) = held {
                            return Err(make_error(
                                ErrorCode::CommandRejected,
                                format!(
                                    "view {id} hides '{held}', which holds '{layer}': show \
                                     '{held}' (VIEWS SHOW {id} {held})"
                                ),
                                layer.as_str(),
                            ));
                        }
                    }
                }
                for layer in &layers {
                    if action == "HIDE" {
                        let _ = view.layers.hide(layer);
                    } else {
                        let _ = view.layers.show(layer);
                    }
                }
            }
        }
        host.settings_changed(id);
        return record_of(host, id);
    }
    if action == "SET" {
        // A view's own switches, as its Layers popup sets them. Every word is
        // read before any is applied, so a refused line changes nothing.
        const SET_USAGE: &str = "VIEWS SET <id> ghosts=on|off";
        if args.len() < 3 {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                format!("VIEWS SET takes a view id and what to set: {SET_USAGE}"),
            ));
        }
        let id = parse_view_id(&args[1])?;
        let mut ghosts = match host.views().borrow().find(id) {
            Some(view) => view.selection_ghosts,
            None => return Err(not_open(id)),
        };
        for word in &args[2..] {
            let Some(equals) = word
                .find('=')
                .filter(|&equals| upper(&word[..equals]) == "GHOSTS")
            else {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!("VIEWS SET does not take '{word}': {SET_USAGE}"),
                    word.as_str(),
                ));
            };
            let value = upper(&word[equals + 1..]);
            if value != "ON" && value != "OFF" {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!("ghosts= is on or off, not '{}'", &word[equals + 1..]),
                    word.as_str(),
                ));
            }
            ghosts = value == "ON";
        }
        if let Some(view) = host.views().borrow_mut().find_mut(id) {
            view.selection_ghosts = ghosts;
        }
        host.settings_changed(id);
        return record_of(host, id);
    }
    Err(refuse_views_word(&args[0]))
}

// ---- ZOOM ----------------------------------------------------------------------------------

fn parse_point(word: &str) -> Option<Point2> {
    let comma = word.find(',')?;
    let x = parse_finite_double(&word[..comma])?;
    let y = parse_finite_double(&word[comma + 1..])?;
    Some(Point2::new(x, y))
}

// "x0,y0,x1,y1" in one word, or "x0,y0 x1,y1" in two: the corners in either
// order. `at` is left after what was read.
fn parse_window(args: &[String], at: &mut usize) -> Result<Box2> {
    let start = *at;
    let refuse = || {
        make_error(
            ErrorCode::ParseFailure,
            "ZOOM WINDOW takes two corners: x0,y0,x1,y1",
            args.get(start).cloned().unwrap_or_default(),
        )
    };
    let Some(word) = args.get(start) else {
        return Err(refuse());
    };
    let mut numbers = Vec::new();
    for part in word.split(',') {
        let Some(value) = parse_finite_double(part) else {
            return Err(refuse());
        };
        numbers.push(value);
    }
    let mut used = 1;
    if numbers.len() == 2 && start + 1 < args.len() {
        let Some(second) = parse_point(&args[start + 1]) else {
            return Err(refuse());
        };
        numbers.push(second.x);
        numbers.push(second.y);
        used = 2;
    }
    let [x0, y0, x1, y1] = numbers[..] else {
        return Err(refuse());
    };
    let a = Point2::new(x0, y0);
    let b = Point2::new(x1, y1);
    if a == b {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a window needs two different corners",
            word.as_str(),
        ));
    }
    *at += used;
    Ok(Box2::new(
        Point2::new(cpp::min(a.x, b.x), cpp::min(a.y, b.y)),
        Point2::new(cpp::max(a.x, b.x), cpp::max(a.y, b.y)),
    ))
}

// A zoom factor: finite and above 0.
fn parse_factor(word: &str) -> Result<f64> {
    parse_finite_double(word)
        .filter(|&factor| factor > 0.0)
        .ok_or_else(|| {
            make_error(
                ErrorCode::ParseFailure,
                "a zoom factor is a number above 0: above 1 is closer, below 1 farther",
                word,
            )
        })
}

// A factor as AutoCAD writes one relative to the view, "2X" or "0.5x", is a
// bare factor here, which is always relative to the view: the number, else
// the word as it is. ("2XP", relative to paper space, stays a word ZOOM
// refuses: a plan view has no paper space.)
fn times_view(word: &str) -> &str {
    if word.len() > 1 && (word.ends_with('X') || word.ends_with('x')) {
        return &word[..word.len() - 1];
    }
    word
}

// Of what a scope took, what `view` shows, in the order given: the entities
// it draws - by its own layers as well as the drawing's (is_drawn) - and, with
// its ghosts on, the selected ones it ghosts (is_ghost). Not a label ghosted
// in a plan view: the plan painter places labels among the others and ghosts
// none (PlanPainter::drawSelectionGhosts), where a 3D view marks where one
// attaches as it marks a ghosted text.
fn shown_of(document: &Document, view: &ViewState, ids: &[EntityId]) -> Vec<EntityId> {
    let model = document.model();
    let mut shown = Vec::new();
    for &id in ids {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        let label_in_plan =
            view.kind == ViewKind::Plan && matches!(entity.geometry, Geometry::Label(_));
        let ghosted = view.selection_ghosts
            && document.selection().contains(id)
            && is_ghost(model, entity, &view.layers)
            && !label_in_plan;
        if ghosted || is_drawn(model, entity, &view.layers) {
            shown.push(id);
        }
    }
    shown
}

// The word the refusal of a kind names the request by.
fn request_word(kind: ZoomRequestKind) -> &'static str {
    match kind {
        ZoomRequestKind::Extents => "EXTENTS",
        ZoomRequestKind::In => "IN",
        ZoomRequestKind::Out => "OUT",
        ZoomRequestKind::Factor => "by a factor",
        ZoomRequestKind::Window => "WINDOW",
        ZoomRequestKind::Centre => "CENTRE",
        ZoomRequestKind::Scope => "on a scope",
    }
}

fn refuse_zoom_word(word: &str) -> Error {
    make_error(
        ErrorCode::ParseFailure,
        format!("ZOOM does not take '{word}': {ZOOM_USAGE}"),
        word,
    )
}

fn zoom_verb(
    document: &Document,
    host: &dyn ViewVerbHost,
    args: &[String],
    scope_views: ScopeViewProvider<'_>,
) -> Result<String> {
    let mut args: Vec<String> = args.to_vec();
    let mut request = ZoomRequest::default();

    // view= first, wherever it stands, so nothing after reads it as a word of
    // its own - after WHERE it would be taken for a condition and refused.
    let mut named: Option<ViewId> = None;
    let mut index = 0;
    while index < args.len() {
        let word = &args[index];
        if word.len() >= 5 && equals_ignoring_case(&word.as_bytes()[..5], b"view=") {
            if named.is_some() {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "give view= once",
                    word.as_str(),
                ));
            }
            named = Some(parse_view_id(&word[5..])?);
            args.remove(index);
        } else {
            index += 1;
        }
    }
    request.view = {
        let views = host.views().borrow();
        match named {
            Some(named) => {
                if views.find(named).is_none() {
                    return Err(not_open(named));
                }
                named
            }
            None => {
                let active = views.active_id();
                if active == NO_VIEW {
                    return Err(Error::new(
                        ErrorCode::InvalidState,
                        "no view is open to zoom: VIEWS OPEN plan opens one",
                    ));
                }
                active
            }
        }
    };

    let mut at = 0;
    // What a scope took, when the line gave one.
    let mut scope: Option<ScopeMatch> = None;
    // ZOOM A and ZOOM ALL alone are Zoom All, the extents - every CAD
    // program's, and the zoom line a script most often holds; on the old
    // window it zoomed the extents whatever followed ZOOM, so a refusal here
    // stopped a script that ran to the end there. ALL is the shared scope word
    // for the drawing too, and is that with a filter after it (ZOOM ALL WHERE
    // ...), which frames the entities it matches.
    let zoom_all = args.len() == 1 && matches!(upper(&args[0]).as_str(), "A" | "ALL");
    if !args.is_empty() && is_scope_word(&args[0]) && !zoom_all {
        // The one parser and the one matcher every verb on drawing data uses.
        // Only with a scope word: ZOOM alone is EXTENTS, where no scope word
        // elsewhere means the selection. No view is borrowed here: the
        // provider may read the host's views to answer VIEW.
        let words = parse_scope_words(&args, &mut at)?;
        if let Some(word) = args.get(at) {
            return Err(refuse_zoom_word(word));
        }
        scope = Some(match_scope(document, &words, scope_views)?);
        // What the view shows of it - and so its box - once the view is
        // known to take a scope (below).
        request.kind = ZoomRequestKind::Scope;
    } else if !args.is_empty() {
        let word = upper(&args[0]);
        at = 1;
        if word == "EXTENTS" || word == "E" || zoom_all {
            request.kind = ZoomRequestKind::Extents;
        } else if word == "IN" || word == "OUT" {
            request.kind = if word == "IN" {
                ZoomRequestKind::In
            } else {
                ZoomRequestKind::Out
            };
            if at < args.len() && parse_finite_double(&args[at]).is_some() {
                request.factor = parse_factor(&args[at])?;
                at += 1;
            }
        } else if word == "WINDOW" || word == "W" {
            request.kind = ZoomRequestKind::Window;
            request.window = parse_window(&args, &mut at)?;
        } else if word == "CENTRE" || word == "CENTER" || word == "C" {
            request.kind = ZoomRequestKind::Centre;
            let Some(centre) = args.get(at).and_then(|word| parse_point(word.as_str())) else {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "ZOOM CENTRE takes the point to centre on: x,y",
                    args.get(at).cloned().unwrap_or_default(),
                ));
            };
            request.centre = centre;
            at += 1;
            if at < args.len() && upper(&args[at]) == "SCALE" {
                let scale = args
                    .get(at + 1)
                    .and_then(|word| parse_finite_double(word.as_str()));
                let Some(scale) = scale.filter(|scale| {
                    (ViewTransform::MINIMUM_SCALE..=ViewTransform::MAXIMUM_SCALE).contains(scale)
                }) else {
                    // The limits a plan view clamps to, read from there so the
                    // message cannot drift from them.
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        format!(
                            "SCALE is pixels per unit from {} to {}",
                            real(ViewTransform::MINIMUM_SCALE),
                            real(ViewTransform::MAXIMUM_SCALE)
                        ),
                        args.get(at + 1).unwrap_or(&args[at]).as_str(),
                    ));
                };
                request.scale = Some(scale);
                at += 2;
            }
        } else {
            let number = times_view(&args[0]);
            if parse_finite_double(number).is_some() {
                request.kind = ZoomRequestKind::Factor;
                request.factor = parse_factor(number)?;
            } else {
                return Err(refuse_zoom_word(&args[0]));
            }
        }
        if let Some(word) = args.get(at) {
            return Err(refuse_zoom_word(word));
        }
    }

    let Some(kind) = host
        .views()
        .borrow()
        .find(request.view)
        .map(|view| view.kind)
    else {
        return Err(not_open(request.view));
    };
    if !zoom_takes(kind, request.kind) {
        // The kinds that take it, read from zoom_takes itself so the message
        // cannot drift from the rule. A plan view takes every request, so the
        // list is never empty and "a plan" leads it.
        let takers: Vec<String> = [
            ViewKind::Plan,
            ViewKind::Model3D,
            ViewKind::Elevation,
            ViewKind::Section,
        ]
        .into_iter()
        .filter(|&taker| zoom_takes(taker, request.kind))
        .map(|taker| {
            if taker == ViewKind::Model3D {
                String::from("3D")
            } else {
                kind_word(taker)
            }
        })
        .collect();
        let mut which = format!("a {}", takers[0]);
        for (i, taker) in takers.iter().enumerate().skip(1) {
            which += if i + 1 == takers.len() { " or " } else { ", " };
            which += taker;
        }
        let does = match request.kind {
            ZoomRequestKind::Window | ZoomRequestKind::Scope => "frames",
            ZoomRequestKind::Centre => "centres",
            _ => "zooms",
        };
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "ZOOM {} {does} {which} view: view {} is {} (ZOOM EXTENTS frames any view)",
                request_word(request.kind),
                request.view,
                kind.name()
            ),
            format!("{}", request.view),
        ));
    }

    let mut reply = String::new();
    if let Some(scope) = &scope {
        reply = scope_record(scope);
        // Taking nothing is an answer, not a refusal: nothing to frame, and no
        // view moved.
        if scope.matched.is_empty() {
            return Ok(reply);
        }
        // The view frames what it shows of what the scope took, in every kind
        // of view: a plan view framed the box of all of it, and jumped to
        // empty ground - and took its link there - for a selection on a layer
        // it hides with its ghosts off, or one the drawing hides, where a 3D
        // view frames what it draws and stays put.
        let views = host.views().borrow();
        let Some(view) = views.find(request.view) else {
            return Err(not_open(request.view));
        };
        request.ids = shown_of(document, view, &scope.matched);
        request.window = extent_of(document.model(), &request.ids);
    }
    // A view that shows none of what the scope took does not move, and says so
    // after the scope's record: the scope took something, so the scope's record
    // alone read as though it had been framed.
    let unmoved = |reply: &str, shown: usize| {
        format!(
            "{reply}\nview={} kind={} shown={shown} moved=no",
            request.view,
            kind_word(kind)
        )
    };
    if scope.is_some() && request.ids.is_empty() {
        return Ok(unmoved(&reply, 0));
    }
    let moved = host.zoom(&request)?;
    if moved.is_empty() {
        // What the view shows of it gave its widget nothing to frame:
        // construction lines alone, which no extent counts.
        return Ok(unmoved(&reply, request.ids.len()));
    }
    let views = host.views().borrow();
    for id in moved {
        let Some(each) = views.find(id) else {
            continue;
        };
        if !reply.is_empty() {
            reply.push('\n');
        }
        reply += &format!("view={id} kind={}", kind_word(each.kind));
        if id != request.view {
            reply += &format!(" followed={}", request.view);
        }
        reply += &place_of(each, false);
        if scope.is_some() && id == request.view {
            // How many of what the scope took it framed: fewer than matched
            // where it hides some of them and ghosts none.
            reply += &format!(" shown={}", request.ids.len());
        }
    }
    Ok(reply)
}
