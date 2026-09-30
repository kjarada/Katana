#pragma once

// VIEWS and ZOOM: the window's views on the command line (the owner's request
// of 2026-09-30 for views that pan and zoom together, with their controls on
// each view's own bar; docs/cad.md, "The window's views: VIEWS and ZOOM").
//
// A control on a view's bar does not move the view itself: it builds one of
// these lines and hands it to the window's command runner, so a click, a line
// typed, a script and an agent driving `katana --command` reach the views by
// ONE path, logged as a typed line is. The Link button runs VIEWS LINK and
// VIEWS UNLINK.
//
// The Document holds no views - the window's workspace does, in a ViewSet -
// so the verbs reach them through a ViewVerbHost the front end supplies, as
// the scope word VIEW reaches its view through a ScopeViewProvider. katana_cli
// and katana_mcp have no views, and there both verbs are refused by name,
// never as unknown commands.
//
//   VIEWS [LIST]                          a record per open view, in the order
//                                         they were opened; then views=N linked=M
//   VIEWS OPEN plan|3d|section|elevation  opens one and makes it active
//   VIEWS ACTIVATE <id>
//   VIEWS LINK <id>[,<id>...] [TO <id>]   leader=1 linked=1,2 moved=2
//   VIEWS UNLINK <id>[,<id>...] | ALL     unlinked=1,2 linked=none
//   VIEWS HIDE <id> <layer>[,<layer>...]  the layers that view hides of its own
//   VIEWS SHOW <id> <layer>[,<layer>...] | ALL
//   VIEWS ISOLATE <id> <layer>            that layer, its parents and what lies
//                                         beneath it, and nothing else
//   VIEWS SET <id> ghosts=on|off          whether that view shows the selection
//                                         faintly where it hides the layer
//   ZOOM | Z | 'ZOOM | 'Z                 the active view's extents, as always
//   ZOOM EXTENTS | E | ALL | A | IN [f] | OUT [f] | <f> | <f>X | WINDOW x0,y0,x1,y1
//        | CENTRE x,y [SCALE s]  [view=<id>]
//   ZOOM SELECTION | DRAWING | VIEW [<id>] [EXTENTS] | AREA x0,y0,x1,y1
//        | LAYERS a,b [ONLY]  [WHERE key=value ...]  [view=<id>]
//
// An id is the window's ViewId - what a record's view= says - and never the
// number in a view's title ("Plan 2" is numbered among the plan views, its id
// among every view opened). view= may stand anywhere on a ZOOM line and is
// taken off before anything else is read, so a WHERE never reads it as a
// condition; without it ZOOM acts on the active view. Linked views follow
// every ZOOM.
//
// EXTENTS frames what the view draws, in any kind of view; ALL and A alone
// are the same, every CAD program's Zoom All (with a filter after it, ALL is
// the scope word for the whole drawing). IN and OUT (by 2 unless a factor is
// given) and a bare factor zoom about the view's centre, in a plan view or a
// section - a factor may carry AutoCAD's X, "2X", relative to the view as a
// bare one always is; WINDOW frames a box as Zoom Extents frames the
// drawing - a box of the drawing, where the scope word AREA would be the
// entities found in one; CENTRE puts a point in the middle, at SCALE pixels
// per unit when given. WINDOW and CENTRE act on plan views. 'ZOOM and 'Z,
// AutoCAD's transparent form, are ZOOM whether or not a tool runs.
//
// The scope words are the shared grammar (scope_verbs.hpp), read by the one
// parser and resolved by matchScope: ZOOM frames what they take, by
// extentOf. ZOOM alone is still EXTENTS, where no scope word elsewhere is the
// selection. The reply begins with the scope record; a scope that takes
// nothing moves no view and says so (matched=0), which is not a refusal. A
// scope frames a plan view; a 3D view's frame of what a scope takes is not
// done (docs/cad.md), and 3D and elevation views take EXTENTS alone until
// their zoom towards the cursor is merged.
//
// HIDE, SHOW and ISOLATE are the view's own layer filter, its Layers button's
// (cad::LayerOverrides): subtractive, so a view never shows what the document
// hides, and like the rest of a view's state not saved and not undoable. A
// layer is a layer of the drawing or a node of the layer tree above one
// ("design" when only "design/road" is a layer); one the drawing lacks is
// refused, naming it, and nothing changes. SHOW of a layer beneath one the
// view hides is refused, naming what holds it: it would stay hidden. The
// reply is the view's record.
//
// Records, one per line, key=value, reals as core::formatExactReal writes
// them and angles in degrees; ghosts= is the view's switch (VIEWS SET), and
// hidden= the layers a view hides of its own, left out when it hides none:
//
//   view=1 kind=plan title="Plan 1" active=yes linked=yes centre=50,40 scale=8 area=x0,y0,x1,y1
//          ghosts=on hidden=asbuilt
//   view=3 kind=3d title="3D 1" active=no target=x,y,z distance=d azimuth=a elevation=e
//          projection=perspective ghosts=on
//   view=4 kind=section title="Section 1" active=no ghosts=on
//
// ZOOM replies the record of the view it moved without title, active,
// linked, ghosts and hidden - where it looks, not what it shows - then a line
// for each view that followed it:
//
//   view=2 kind=plan followed=1 centre=50,40 scale=8 area=x0,y0,x1,y1
//
// and with scope words, the scope's record first:
//
//   scope=selection matched=1
//   view=2 kind=plan centre=25,20 scale=... area=...

#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// What ZOOM asks of one view.
struct ZoomRequest {
    enum class Kind {
        Extents, // what the view draws: its own Zoom Extents
        In,      // `factor` times closer, about the view's centre
        Out,     // `factor` times farther, about the view's centre
        Factor,  // times `factor` about the view's centre: above 1 is closer
        Window,  // frames `window` as Zoom Extents frames the drawing
        Centre,  // `centre` in the middle, at `scale` pixels per unit if given
        // Frames `window`, the extent of what a scope took (extentOf): framed
        // as WINDOW frames a box in a plan view; kept apart because a view of
        // another kind would frame the entities, not a box.
        Scope,
    };
    ViewId view = kNoView;
    Kind kind = Kind::Extents;
    // In, Out and Factor: finite and above 0.
    double factor = 2.0;
    katana::geometry::Box2 window{};
    katana::geometry::Point2 centre{};
    // Within ViewTransform's limits: SCALE outside them is refused, not
    // clamped, because a scale the view cannot show is not the one asked for.
    std::optional<double> scale{};
};

// Whether a view of `kind` takes `request`: EXTENTS any view; IN, OUT and a
// factor a plan view and a section, which zoom about their centres; WINDOW,
// CENTRE and a scope a plan view, whose plan position they are. A 3D or
// elevation view zooms by the wheel towards what is under the cursor, which
// ZOOM IN is to use at its centre once that zoom is merged (docs/cad.md).
// The one rule: ZOOM refuses by it, and a view's bar, the View menu and the
// Zoom To dialog offer only what it allows, so widening it widens all four.
[[nodiscard]] bool zoomTakes(ViewKind kind, ZoomRequest::Kind request);

// In, Out, Factor and Centre applied to a plan view's transform: arithmetic
// about the view's centre, which needs neither the widget nor its size. The
// scale is clamped to ViewTransform's limits as the wheel's is. Window and
// Extents are the widget's - they fit what it draws, at its size, with its
// margin - and return false here, the transform untouched.
bool applyPlanZoom(ViewTransform& view, const ZoomRequest& request);

// The window's side: what the verbs need of the views the Document does not
// hold. MainWindow's workspace answers it; a test answers with a ViewSet of
// its own.
class ViewVerbHost {
  public:
    ViewVerbHost() = default;
    ViewVerbHost(const ViewVerbHost&) = delete;
    ViewVerbHost& operator=(const ViewVerbHost&) = delete;
    virtual ~ViewVerbHost() = default;

    // The open views: the verbs read them, and link and unlink them, here.
    [[nodiscard]] virtual ViewSet& views() = 0;
    // Opens a view of `kind` and makes it the active one: its id.
    [[nodiscard]] virtual katana::core::Result<ViewId> open(ViewKind kind) = 0;
    // NotFound for an id that is not open.
    [[nodiscard]] virtual katana::core::Status activate(ViewId id) = 0;
    // Moves `request.view` as asked, through its widget, and then every view
    // linked with it: the view moved, then each view that followed it, in
    // the order they were opened. The request has been checked against the
    // view's kind; a host fails only with what its widget refuses.
    [[nodiscard]] virtual katana::core::Result<std::vector<ViewId>>
    zoom(const ZoomRequest& request) = 0;
    // The verbs changed the link: these views joined or left it, and may have
    // been moved behind their widgets' backs. Repaint them, keep a linked view
    // from refitting its old frame on a resize, and bring every view's bar up
    // to date.
    virtual void changed(const std::vector<ViewId>& ids) = 0;
    // The verbs changed what `id` draws of its own - the layers it hides
    // (ViewState::layers) or its ghosts of the selection (selectionGhosts):
    // no document change, so nothing else hears of it. Redraw that view and
    // bring its Layers button up to date.
    virtual void settingsChanged(ViewId id) = 0;
};

// Asked at each VIEWS or ZOOM line, so a front end can hand over a host that
// did not exist when the interpreter was made. Unset, or answering null, is
// no window: both verbs are refused by name.
using ViewHostProvider = std::function<ViewVerbHost*()>;

// VIEWS or ZOOM, after the interpreter's aliases (Z is ZOOM).
[[nodiscard]] bool isViewVerb(std::string_view verb);

// Runs one VIEWS or ZOOM line: `verb` as isViewVerb takes it, `args` the words
// after it as the interpreter splits them. `scopeViews` answers the scope word
// VIEW, as it does for every verb that takes the shared scope. The reply is
// the records above; refusals name the word, the view or the kind at fault.
[[nodiscard]] katana::core::Result<std::string>
runViewVerb(const Document& document, std::string_view verb, const std::vector<std::string>& args,
            const ViewHostProvider& host, const ScopeViewProvider& scopeViews = {});

// The record VIEWS gives for `view`, a view of `views`.
[[nodiscard]] std::string viewRecord(const ViewSet& views, const ViewState& view);

} // namespace katana::cad
