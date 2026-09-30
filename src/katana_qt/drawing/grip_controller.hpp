#pragma once

// The plan view's grips (docs/drawing.md, "Grips"): with entities selected
// and no tool running, a square on every vertex, segment middle, centre,
// quadrant and insertion point, which the user hovers, clicks and drags.
//
// What a grip IS and what dragging it DOES are katana_cad's
// (cad/drawing/grips.hpp); this is only the state of the gesture and the
// drawing of the squares, so the view forwards its mouse and keys here
// before its own selection handling:
//
//   press on a grip        grab it; plain: it and only it hot, unless it was
//                          already hot (then every hot grip moves); Shift:
//                          toggles it hot without grabbing
//   drag, release          the hot grips move with the cursor - snapped,
//                          tracked, ortho or polar as the view constrains it
//                          - and the release is ONE undoable GRIP_EDIT
//   click, move, click     the same without holding the button (the grip is
//                          picked up by a click and put down by the next)
//   typed while grabbed    x,y / @dx,dy / @distance<angle or bearing / a
//                          plain distance along the cursor, then Enter
//   Delete                 removes the hot vertex grips' vertices (one step)
//   Esc                    drops a grabbed grip, then the hot set
//
// Everything is in model coordinates except the aperture tests, which the
// view converts; nothing here knows the view's transform.

#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <vector>

#include <QColor>
#include <QPointF>
#include <QRectF>
#include <QString>

#include "katana/cad/document.hpp"
#include "katana/cad/drawing/grips.hpp"

class QPainter;

namespace katana::qt::drawing {

class GripController {
  public:
    using Point2 = katana::geometry::Point2;

    explicit GripController(katana::cad::Document& document);

    // Rebuilds the grips from the document's selection when `generation`
    // (the view's count of document notifications) has moved on.
    void refresh(std::uint64_t generation);
    [[nodiscard]] const std::vector<katana::cad::Grip>& grips() const { return grips_; }
    [[nodiscard]] const std::vector<katana::cad::Grip>& hot() const { return hot_; }
    [[nodiscard]] std::optional<katana::cad::Grip> hovered() const { return hovered_; }

    // True while a grip is held (dragged) or picked up: the view then snaps
    // and constrains the cursor from base() and sends every move here.
    [[nodiscard]] bool active() const { return state_ == State::Dragging || state_ == State::Attached; }
    [[nodiscard]] bool pressed() const { return state_ == State::Pressed; }
    // Where the grabbed grip was: the base of the drag's rubber band, of
    // relative typed input and of the ortho and polar constraints.
    [[nodiscard]] std::optional<Point2> base() const;
    [[nodiscard]] const QString& typed() const { return typed_; }
    [[nodiscard]] QString prompt() const;

    // ---- what the view forwards; each returns true when it took the input ----
    // A left press at `at` (model), `aperture` the grip's reach in model
    // units; Ctrl on a segment middle inserts a vertex instead of stretching.
    bool press(const Point2& at, double aperture, bool shift, bool ctrl);
    // The cursor, already snapped and constrained by the view. `movedPixels`
    // is how far the mouse has gone since the press, to tell a drag from a
    // click.
    void move(const Point2& target, const Point2& raw, double aperture, double movedPixels);
    bool release();
    bool escape();
    bool deleteHot();
    bool enter();
    bool type(const QString& text);
    bool backspace();
    // Drops everything: a new drawing, a tool starting.
    void reset();
    // The layers the view hides of its own (ViewState::layers), which
    // outlive this: a selected entity on one of them offers no grip here
    // (cad::gripsOfSelection) - it is a ghost in this view at most. Null,
    // the default, is the document's rule alone. They change with no
    // document notification, so refresh() compares them with what it last
    // built from.
    void setView(const katana::cad::LayerOverrides* view) { view_ = view; }

    // The grips, the drag's preview and its rubber band, onto `painter`
    // through `toScreen`; only grips inside `visible` (screen pixels).
    void paint(QPainter& painter, const std::function<QPointF(const Point2&)>& toScreen,
               const std::function<void(const katana::entity::Geometry&)>& drawShape,
               const QRectF& visible) const;

    std::function<void(const QString&)> onError;
    std::function<void(const QString&)> onMessage;

  private:
    enum class State { Idle, Pressed, Dragging, Attached };

    void commitAt(const Point2& target);
    [[nodiscard]] katana::cad::GripDrag dragTo(const Point2& target) const;
    [[nodiscard]] bool isHot(const katana::cad::Grip& grip) const;

    katana::cad::Document& document_;
    const katana::cad::LayerOverrides* view_ = nullptr;
    // What view_ held when the grips were last built.
    katana::cad::LayerOverrides builtFor_;
    std::uint64_t generation_ = ~std::uint64_t{0};
    std::vector<katana::cad::Grip> grips_;
    std::vector<katana::cad::Grip> hot_;
    std::optional<katana::cad::Grip> hovered_;
    std::optional<katana::cad::Grip> grabbed_;
    State state_ = State::Idle;
    bool insert_ = false;
    Point2 target_;
    Point2 rawCursor_;
    QString typed_;
};

} // namespace katana::qt::drawing
