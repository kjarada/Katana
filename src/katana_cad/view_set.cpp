#include "katana/cad/view_set.hpp"

#include <algorithm>

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

ViewState& ViewSet::add(ViewKind kind)
{
    auto view = std::make_unique<ViewState>();
    view->id = next_++;
    view->kind = kind;
    view->number = lowestFreeNumber(kind, view->id);
    configureCamera(view->camera, kind);
    view->cameraKind = kind;
    ViewState& added = *view;
    views_.push_back(std::move(view));
    // Least recent: a view nobody has clicked yet must not outrank one the
    // user was just working in.
    recent_.insert(recent_.begin(), added.id);
    if (active_ == kNoView) {
        active_ = added.id;
        touch(added.id);
    }
    return added;
}

Result<ViewState*> ViewSet::restore(ViewId id, ViewKind kind)
{
    if (id == kNoView) {
        return makeError(ErrorCode::InvalidArgument, "a view id cannot be zero");
    }
    if (find(id) != nullptr) {
        return makeError(ErrorCode::AlreadyExists, "a view with this id is already open",
                         std::to_string(id));
    }
    const ViewId saved = next_;
    next_ = id;
    ViewState& view = add(kind);
    next_ = std::max(saved, id + 1);
    return &view;
}

Status ViewSet::remove(ViewId id)
{
    if (take(id) == nullptr) {
        return makeError(ErrorCode::NotFound, "no such view", std::to_string(id));
    }
    return {};
}

std::unique_ptr<ViewState> ViewSet::take(ViewId id)
{
    const auto found = std::ranges::find_if(
        views_, [&](const std::unique_ptr<ViewState>& view) { return view->id == id; });
    if (found == views_.end()) {
        return nullptr;
    }
    std::unique_ptr<ViewState> taken = std::move(*found);
    views_.erase(found);
    std::erase(recent_, id);
    if (active_ == id) {
        active_ = recent_.empty() ? kNoView : recent_.back();
    }
    return taken;
}

Status ViewSet::setKind(ViewId id, ViewKind kind)
{
    ViewState* view = find(id);
    if (view == nullptr) {
        return makeError(ErrorCode::NotFound, "no such view", std::to_string(id));
    }
    if (view->kind == kind) {
        return {};
    }
    view->kind = kind;
    view->number = lowestFreeNumber(kind, id);
    // Section does not look through the camera, so it leaves it; and arriving
    // back at the kind the camera is already set up for leaves it too. The
    // second half is what makes configureCamera's promise - a trip through a
    // section restores the view - true: resetting on every change of kind
    // lost a 3D view's orbit on the way back (ViewSet.AThreeDViewTurnedInto
    // ASectionAndBackKeepsItsOrbit).
    if (kind != ViewKind::Section && kind != view->cameraKind) {
        configureCamera(view->camera, kind);
        view->cameraKind = kind;
        view->cameraFramed = false;
    }
    return {};
}

Status ViewSet::activate(ViewId id)
{
    if (find(id) == nullptr) {
        return makeError(ErrorCode::NotFound, "no such view", std::to_string(id));
    }
    active_ = id;
    touch(id);
    return {};
}

ViewState* ViewSet::find(ViewId id)
{
    const auto found = std::ranges::find_if(
        views_, [&](const std::unique_ptr<ViewState>& view) { return view->id == id; });
    return found == views_.end() ? nullptr : found->get();
}

const ViewState* ViewSet::find(ViewId id) const
{
    const auto found = std::ranges::find_if(
        views_, [&](const std::unique_ptr<ViewState>& view) { return view->id == id; });
    return found == views_.end() ? nullptr : found->get();
}

ViewState* ViewSet::active() { return find(active_); }

ViewState* ViewSet::mostRecent(ViewKind kind)
{
    for (auto at = recent_.rbegin(); at != recent_.rend(); ++at) {
        ViewState* view = find(*at);
        if (view != nullptr && view->kind == kind) {
            return view;
        }
    }
    return nullptr;
}

std::vector<ViewState*> ViewSet::views()
{
    std::vector<ViewState*> out;
    out.reserve(views_.size());
    for (const auto& view : views_) {
        out.push_back(view.get());
    }
    return out;
}

std::vector<const ViewState*> ViewSet::views() const
{
    std::vector<const ViewState*> out;
    out.reserve(views_.size());
    for (const auto& view : views_) {
        out.push_back(view.get());
    }
    return out;
}

std::size_t ViewSet::count(ViewKind kind) const
{
    return static_cast<std::size_t>(std::ranges::count_if(
        views_, [&](const std::unique_ptr<ViewState>& view) { return view->kind == kind; }));
}

std::string ViewSet::title(const ViewState& view)
{
    return std::string(toString(view.kind)) + " " + std::to_string(view.number);
}

int ViewSet::lowestFreeNumber(ViewKind kind, ViewId except) const
{
    int number = 1;
    while (std::ranges::any_of(views_, [&](const std::unique_ptr<ViewState>& view) {
        return view->id != except && view->kind == kind && view->number == number;
    })) {
        ++number;
    }
    return number;
}

void ViewSet::touch(ViewId id)
{
    std::erase(recent_, id);
    recent_.push_back(id);
}

} // namespace katana::cad
