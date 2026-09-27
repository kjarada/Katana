// A point picked in a plan view (point_pick.hpp).

#include "geo/point_pick.hpp"

#include <QEvent>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QObject>
#include <QPointer>

#include <memory>
#include <utility>
#include <vector>

#include "view_workspace.hpp"
#include "viewport_widget.hpp"

namespace katana::qt {

namespace {

using katana::geometry::Point2;

// Watches every plan view for the one click; removes itself when it has it.
// Built on an event filter rather than a catalogue tool: a tool runs in one
// view's ToolHost and ends in a command, and a pick is neither - it is any
// view's next click, and it changes nothing.
class PickFilter final : public QObject {
  public:
    PickFilter(std::vector<QPointer<ViewportWidget>> views, PickedPoint picked)
        : views_(std::move(views)), picked_(std::move(picked))
    {
        for (const QPointer<ViewportWidget>& view : views_) {
            if (view) {
                view->installEventFilter(this);
            }
        }
    }

    // Ends the pick: with `point`, or none. Once.
    void finish(std::optional<Point2> point)
    {
        if (done_) {
            return;
        }
        done_ = true;
        for (const QPointer<ViewportWidget>& view : views_) {
            if (view) {
                view->removeEventFilter(this);
            }
        }
        PickedPoint picked = std::move(picked_);
        deleteLater();
        if (picked) {
            picked(point);
        }
    }

  protected:
    bool eventFilter(QObject* watched, QEvent* event) override
    {
        auto* view = dynamic_cast<ViewportWidget*>(watched);
        if (view == nullptr || done_) {
            return false;
        }
        switch (event->type()) {
        case QEvent::MouseButtonPress: {
            const auto* mouse = static_cast<QMouseEvent*>(event);
            if (mouse->button() == Qt::LeftButton) {
                const QPointF at = mouse->position();
                const Point2 world = view->viewTransform().screenToWorld(Point2(at.x(), at.y()));
                finish(world);
                return true;
            }
            if (mouse->button() == Qt::RightButton) {
                finish(std::nullopt);
                return true;
            }
            return false;
        }
        case QEvent::KeyPress:
            if (static_cast<QKeyEvent*>(event)->key() == Qt::Key_Escape) {
                finish(std::nullopt);
                return true;
            }
            return false;
        default:
            return false;
        }
    }

  private:
    std::vector<QPointer<ViewportWidget>> views_;
    PickedPoint picked_;
    bool done_ = false;
};

} // namespace

PointPicker planPointPicker(ViewWorkspace& views)
{
    // The pick waiting, if any: a second request ends it first.
    auto waiting = std::make_shared<QPointer<PickFilter>>();
    return [&views, waiting](PickedPoint picked) {
        if (*waiting) {
            (*waiting)->finish(std::nullopt);
        }
        std::vector<QPointer<ViewportWidget>> plans;
        for (ViewportWidget* view : views.planViews()) {
            plans.emplace_back(view);
        }
        if (plans.empty()) {
            if (picked) {
                picked(std::nullopt);
            }
            return;
        }
        *waiting = new PickFilter(std::move(plans), std::move(picked));
    };
}

} // namespace katana::qt
