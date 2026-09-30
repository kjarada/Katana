#include "katana/cad/view_set.hpp"

#include <algorithm>

#include "katana/cad/view_link.hpp"

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
    // Out of the link first, while the view is still one of the set: the
    // members left behind are counted without it.
    (void)leaveLink(**found);
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
    // What it shows now is not what the link keeps in step, and following
    // would move it by numbers that mean something else for its new kind.
    (void)leaveLink(*view);
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

// ---- linked views -------------------------------------------------------------------------

namespace {

bool holds(const std::vector<ViewId>& ids, ViewId id) { return std::ranges::contains(ids, id); }

} // namespace

std::vector<ViewId> ViewSet::linkedViews() const
{
    std::vector<ViewId> members;
    for (const auto& view : views_) {
        if (view->linked) {
            members.push_back(view->id);
        }
    }
    return members;
}

Result<ViewSet::LinkChange> ViewSet::link(std::span<const ViewId> ids, std::optional<ViewId> to)
{
    // Everything is checked before anything changes, so a refused line
    // leaves the link as it was.
    std::vector<ViewId> joining;
    for (const ViewId id : ids) {
        const ViewState* view = find(id);
        if (view == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no view " + std::to_string(id) + " is open (VIEWS lists them)",
                             std::to_string(id));
        }
        if (!linkable(view->kind)) {
            return makeError(ErrorCode::InvalidArgument,
                             "only plan views link: view " + std::to_string(id) + " is " +
                                 toString(view->kind),
                             std::to_string(id));
        }
        if (!holds(joining, id)) {
            joining.push_back(id);
        }
    }
    if (joining.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "name the views to link: VIEWS LINK <id>[,<id>...] [TO <id>]");
    }
    const std::vector<ViewId> members = linkedViews();
    ViewId leader = joining.front();
    if (to) {
        if (find(*to) == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no view " + std::to_string(*to) + " is open (VIEWS lists them)",
                             std::to_string(*to));
        }
        if (!holds(joining, *to) && !holds(members, *to)) {
            return makeError(ErrorCode::InvalidArgument,
                             "TO names the view the others come to, one being linked or already "
                             "linked: view " +
                                 std::to_string(*to) + " is neither",
                             std::to_string(*to));
        }
        leader = *to;
    } else if (!members.empty()) {
        // The link as it stands leads: joining it must not move the views
        // already in it.
        leader = std::ranges::min(members);
    }
    for (const ViewId id : joining) {
        find(id)->linked = true;
    }
    LinkChange change;
    change.leader = leader;
    change.moved = follow(leader);
    change.linked = linkedViews();
    return change;
}

std::vector<ViewId> ViewSet::leaveLink(ViewState& view)
{
    if (!view.linked) {
        return {};
    }
    view.linked = false;
    std::vector<ViewId> left{view.id};
    // A link of one follows nothing and leads nothing; left standing it would
    // make the next view linked follow a view nobody chose. Only a link this
    // left with one member goes: a link of one made on purpose - the first
    // click of two - waits for the second.
    const std::vector<ViewId> members = linkedViews();
    if (members.size() == 1) {
        find(members.front())->linked = false;
        left.push_back(members.front());
    }
    return left;
}

Result<std::vector<ViewId>> ViewSet::unlink(std::span<const ViewId> ids)
{
    for (const ViewId id : ids) {
        if (find(id) == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no view " + std::to_string(id) + " is open (VIEWS lists them)",
                             std::to_string(id));
        }
    }
    std::vector<ViewId> left;
    for (const ViewId id : ids) {
        for (const ViewId gone : leaveLink(*find(id))) {
            if (!holds(left, gone)) {
                left.push_back(gone);
            }
        }
    }
    // In creation order, as every list of views here is.
    std::vector<ViewId> ordered;
    for (const auto& view : views_) {
        if (holds(left, view->id)) {
            ordered.push_back(view->id);
        }
    }
    return ordered;
}

std::vector<ViewId> ViewSet::unlinkAll()
{
    std::vector<ViewId> left = linkedViews();
    for (const ViewId id : left) {
        find(id)->linked = false;
    }
    return left;
}

void ViewSet::noteMoved(ViewId id)
{
    if (ViewState* view = find(id)) {
        view->lastMoved = ++moves_;
    }
}

ViewId ViewSet::linkLeaderFor(ViewId joining) const
{
    const std::vector<ViewId> members = linkedViews();
    ViewId latest = kNoView;
    std::uint64_t latestMove = 0;
    const auto consider = [&](ViewId id) {
        const ViewState* view = find(id);
        if (view != nullptr && view->lastMoved > latestMove) {
            latest = id;
            latestMove = view->lastMoved;
        }
    };
    consider(joining);
    for (const ViewId id : members) {
        consider(id);
    }
    if (latest != kNoView) {
        return latest;
    }
    return members.empty() ? joining : std::ranges::min(members);
}

std::vector<ViewId> ViewSet::follow(ViewId source)
{
    const ViewState* from = find(source);
    // An unframed view's centre and scale are the defaults a widget starts
    // with, not a place anybody chose: nothing follows them.
    if (from == nullptr || !from->linked || !from->planFramed) {
        return {};
    }
    std::vector<ViewId> moved;
    for (const auto& view : views_) {
        if (view->id == source || !view->linked) {
            continue;
        }
        if (followView(*from, *view)) {
            // Framed: the view's widget must not frame it again at its first
            // paint, which would move it off the link.
            view->planFramed = true;
            moved.push_back(view->id);
        }
    }
    return moved;
}

} // namespace katana::cad
