#pragma once

// The open views of the workspace, and what each one is looking at (the
// user's request of 2026-09-23: views that dock, float onto another screen,
// minimise and close, each with its own layers).
//
// This replaces the CELLS of the tiled ViewportLayout. A tile was identified by
// its position in a fixed split, so a view's state lived as long as the split
// did and every layout change rebuilt every widget - the plan zoom, the second
// section and the half-picked clicks all went with it. A docked view is
// identified by an id instead, lives until the user closes it, and can change
// what it shows (plan, 3D, section, elevation) without losing the rest of its
// state. The split presets survive as ARRANGEMENTS of these views
// (viewport_layout.hpp, dockSplits).
//
// Like ViewportLayout before it, this is the tested model: it owns no widgets
// and draws nothing. The Qt workspace hosts one dock per ViewState and gives
// each widget a pointer to its state, which is why states are held by
// unique_ptr - a view's state must not move while its widget is looking at it.

#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <set>
#include <string>
#include <vector>

#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/cad/viewport_layout.hpp"
#include "katana/core/error.hpp"
#include "katana/render/camera.hpp"

namespace katana::cad {

// Monotonic and never reused, like entity ids: a dock that outlives its view
// by a queued event must not find a different view under the same id.
using ViewId = std::uint32_t;
inline constexpr ViewId kNoView = 0;

struct ViewState {
    ViewId id = kNoView;
    ViewKind kind = ViewKind::Plan;
    // Shown in the title: the second plan view open is "Plan 2". The lowest
    // number no other open view of the same kind holds, so closing "Plan 1"
    // and opening another plan gives "Plan 1" again rather than counting up
    // for the whole session.
    int number = 1;

    // Everything below survives a change of kind: switching a view to 3D and
    // back keeps its plan zoom, its hidden layers and its section.
    katana::render::Camera camera; // Model3D and Elevation
    // The kind `camera` was last pointed for by configureCamera. A section
    // draws through its own transform and leaves the camera alone, so a 3D
    // view turned into a section and back finds its orbit where it left it;
    // only arriving at a kind the camera was NOT set up for resets it.
    ViewKind cameraKind = ViewKind::Plan;
    // True once a 3D or elevation view has framed `camera` on something it
    // draws. The widget is rebuilt on every change of kind, and a new one
    // frames at its first paint unless this says the camera already has been:
    // that frame resets the target and the distance, so a trip through a
    // section kept only the angle of the orbit. setKind clears it whenever it
    // points the camera afresh, because a camera pointed afresh has framed
    // nothing yet.
    bool cameraFramed = false;
    ViewTransform plan;            // Plan: pan and zoom
    // False until the plan view has been framed once. The plan widget frames
    // at its first paint, not its first resize, because a dock's first resize
    // may be provisional; and until the user pans or zooms it refits that
    // frame on every resize, so setting this true stops the first frame but
    // not the refit of a later Zoom Extents or zoom-to.
    bool planFramed = false;
    // Layers this view hides on top of the document's own visibility.
    LayerOverrides layers;
    // Reference layers (imported rasters and point clouds) this view hides, by
    // the id the application gave them. cad cannot see interop, where those
    // layers live, so it keeps the ids without interpreting them; they are
    // here only so they survive a change of kind with the rest of the view.
    std::set<std::uint64_t> hiddenReferences;
    // Section: what was cut, and how it is drawn.
    std::optional<Section> section;
    double sectionExaggeration = 10.0;
};

class ViewSet {
  public:
    // A new view with the camera its kind starts with. The first view added to
    // an empty set becomes active; later ones do not, because opening a view
    // and activating it are separate decisions for the workspace.
    ViewState& add(ViewKind kind);

    // Recreates a view under a known id - restoring a saved workspace, whose
    // dock names carry the ids. Fails with InvalidArgument for kNoView and
    // AlreadyExists when the id is open. Later add()s never reuse it.
    [[nodiscard]] katana::core::Result<ViewState*> restore(ViewId id, ViewKind kind);

    // Fails with NotFound for an id that is not open. Removing the active view
    // activates the most recently active of the rest.
    [[nodiscard]] katana::core::Status remove(ViewId id);
    // remove(), handing the state to the caller instead of destroying it; null
    // for an id that is not open. For a widget that is being torn down after
    // its view has closed and still holds a pointer to the state: the state
    // must outlive the widget, and the view must stop being findable at once.
    [[nodiscard]] std::unique_ptr<ViewState> take(ViewId id);

    // Changes what a view shows, and gives the view the lowest free number in
    // its new kind. Reconfigures the camera only for a model kind it was not
    // already set up for (ViewState::cameraKind): a 3D view's orbit is not
    // reset by choosing 3D again, nor by a trip through Section and back.
    // Reconfiguring clears cameraFramed. NotFound for an unknown id.
    [[nodiscard]] katana::core::Status setKind(ViewId id, ViewKind kind);

    // NotFound, and nothing changes, for an unknown id: activating some other
    // view than the one asked for is worse than doing nothing.
    [[nodiscard]] katana::core::Status activate(ViewId id);

    [[nodiscard]] ViewState* find(ViewId id);
    [[nodiscard]] const ViewState* find(ViewId id) const;
    // Null only when the set is empty.
    [[nodiscard]] ViewState* active();
    [[nodiscard]] ViewId activeId() const { return active_; }

    // The active view when it is of `kind`; otherwise the view of that kind
    // that was active most recently; otherwise the first of that kind opened;
    // null when none is open. What Plot, F9 and the Standard Views act on, so
    // they reach the 3D view the user last touched even after they clicked
    // back into the plan.
    [[nodiscard]] ViewState* mostRecent(ViewKind kind);

    // Creation order.
    [[nodiscard]] std::vector<ViewState*> views();
    [[nodiscard]] std::vector<const ViewState*> views() const;
    [[nodiscard]] std::size_t size() const { return views_.size(); }
    [[nodiscard]] bool empty() const { return views_.empty(); }
    [[nodiscard]] std::size_t count(ViewKind kind) const;

    // "Plan 1", "3D 2", "Section 1", "Elevation 1".
    [[nodiscard]] static std::string title(const ViewState& view);

  private:
    [[nodiscard]] int lowestFreeNumber(ViewKind kind, ViewId except) const;
    void touch(ViewId id);

    std::vector<std::unique_ptr<ViewState>> views_;
    // Every open view, least recently active first.
    std::vector<ViewId> recent_;
    ViewId active_ = kNoView;
    ViewId next_ = 1;
};

} // namespace katana::cad
