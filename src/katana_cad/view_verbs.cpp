#include "katana/cad/view_verbs.hpp"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/selection.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/entity/tables.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Box2;
using katana::geometry::Point2;

namespace {

constexpr const char* kViewsUsage =
    "VIEWS [LIST] | OPEN plan|3d|section|elevation | ACTIVATE id | LINK id[,id...] [TO id] | "
    "UNLINK id[,id...]|ALL | HIDE id layer[,layer...] | SHOW id layer[,layer...]|ALL | "
    "ISOLATE id layer | SET id ghosts=on|off";
constexpr const char* kZoomUsage =
    "ZOOM [EXTENTS | IN [f] | OUT [f] | factor | WINDOW x0,y0,x1,y1 | CENTRE x,y [SCALE s] | "
    "SELECTION | DRAWING | VIEW [id] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY] "
    "[WHERE k=v ...]] [view=id]";

using katana::core::uppered;

// The word a record and VIEWS OPEN use for a kind: the title's word, lower
// case, so "3D 1" is kind=3d.
std::string kindWord(ViewKind kind) { return katana::core::lowered(toString(kind)); }

std::optional<ViewKind> kindFrom(std::string_view word)
{
    for (const ViewKind kind :
         {ViewKind::Plan, ViewKind::Model3D, ViewKind::Section, ViewKind::Elevation}) {
        if (katana::core::equalsIgnoringCase(word, toString(kind))) {
            return kind;
        }
    }
    return std::nullopt;
}

std::string real(double value) { return katana::core::formatExactReal(value); }

std::string pointText(const Point2& point) { return real(point.x) + "," + real(point.y); }

std::string listed(const std::vector<ViewId>& ids)
{
    if (ids.empty()) {
        return "none";
    }
    std::string text;
    for (const ViewId id : ids) {
        text += (text.empty() ? "" : ",") + std::to_string(id);
    }
    return text;
}

katana::core::Error notOpen(ViewId id)
{
    return makeError(ErrorCode::NotFound,
                     "no view " + std::to_string(id) + " is open (VIEWS lists them)",
                     std::to_string(id));
}

// A view id as typed: a whole number from 1.
Result<ViewId> parseViewId(std::string_view word)
{
    const bool digits = !word.empty() && std::ranges::all_of(word, [](char c) {
        return c >= '0' && c <= '9';
    });
    const auto value = digits ? katana::core::parseInteger(word) : std::nullopt;
    if (!value || *value < 1 || *value > std::numeric_limits<ViewId>::max()) {
        return makeError(ErrorCode::ParseFailure,
                         "a view id is a whole number from 1: the view= a record gives (VIEWS "
                         "lists them), not the number in a view's title",
                         std::string(word));
    }
    return static_cast<ViewId>(*value);
}

// "1,2" and "1 2", up to TO or the end of the line; each at most once.
Result<std::vector<ViewId>> parseIds(const std::vector<std::string>& args, std::size_t& at)
{
    std::vector<ViewId> ids;
    while (at < args.size() && uppered(args[at]) != "TO") {
        std::string_view word = args[at];
        std::size_t start = 0;
        while (start <= word.size()) {
            const std::size_t comma = std::min(word.find(',', start), word.size());
            // "1,,2" and a trailing comma are stray commas, not ids.
            if (comma > start) {
                auto id = parseViewId(word.substr(start, comma - start));
                if (!id) {
                    return id.error();
                }
                if (!std::ranges::contains(ids, *id)) {
                    ids.push_back(*id);
                }
            }
            start = comma + 1;
        }
        ++at;
    }
    return ids;
}

// The layers of HIDE and SHOW: comma lists in one word or several, each once,
// each a layer of the drawing or a node of the layer tree above one - what
// the Layers popup lists. A name with a blank in it is quoted, as the
// interpreter's words are.
Result<std::vector<std::string>> parseLayers(const Document& document,
                                             const std::vector<std::string>& args,
                                             std::size_t from)
{
    std::vector<std::string> layers;
    for (std::size_t at = from; at < args.size(); ++at) {
        std::string_view word = args[at];
        std::size_t start = 0;
        while (start <= word.size()) {
            const std::size_t comma = std::min(word.find(',', start), word.size());
            if (comma > start) {
                std::string layer(word.substr(start, comma - start));
                if (!std::ranges::contains(layers, layer)) {
                    layers.push_back(std::move(layer));
                }
            }
            start = comma + 1;
        }
    }
    const std::vector<std::string> names = document.model().layers.names();
    for (const std::string& layer : layers) {
        const bool exists = std::ranges::any_of(names, [&layer](const std::string& name) {
            return katana::entity::isLayerUnder(name, layer);
        });
        if (!exists) {
            return makeError(ErrorCode::NotFound,
                             "no layer '" + layer + "' in the drawing (LAYER LIST lists them)",
                             layer);
        }
    }
    return layers;
}

// The layers a view hides of its own, for its record: none, or hidden=a,b.
std::string hiddenOf(const ViewState& view)
{
    if (view.layers.empty()) {
        return {};
    }
    std::string text;
    for (const std::string& layer : view.layers.hidden()) {
        text += (text.empty() ? "" : ",") + layer;
    }
    return " hidden=" + recordValue(text);
}

// Where a view is looking, after its id, kind and (for VIEWS) title and
// active: a plan view's centre, scale and area; a 3D or elevation view's
// camera; nothing for a section, whose pan and zoom its widget keeps. Then
// framed=no while the view has framed nothing: the place is the one every
// new view starts at, and the view's first paint will leave it - an agent
// reading VIEWS OPEN's record took it for where the view looks.
std::string placeOf(const ViewState& view, bool withLinked)
{
    std::string text;
    bool framed = true;
    switch (view.kind) {
    case ViewKind::Plan: {
        if (withLinked) {
            text += std::string(" linked=") + (view.linked ? "yes" : "no");
        }
        const Box2 area = view.plan.visibleWorldBounds();
        text += " centre=" + pointText(view.plan.center) + " scale=" + real(view.plan.scale) +
                " area=" + pointText(area.min) + "," + pointText(area.max);
        framed = view.planFramed;
        break;
    }
    case ViewKind::Model3D:
    case ViewKind::Elevation: {
        const katana::render::Camera& camera = view.camera;
        const katana::geometry::Point3& target = camera.target();
        text += " target=" + real(target.x) + "," + real(target.y) + "," + real(target.z) +
                " distance=" + real(camera.distance()) +
                " azimuth=" + real(camera.azimuth() * katana::math::kRadToDeg) +
                " elevation=" + real(camera.elevation() * katana::math::kRadToDeg) +
                " projection=" +
                (camera.projection() == katana::render::Projection::Perspective ? "perspective"
                                                                                : "orthographic");
        framed = view.cameraFramed;
        break;
    }
    case ViewKind::Section:
        break;
    }
    return framed ? text : text + " framed=no";
}

// ---- VIEWS --------------------------------------------------------------------------------

Result<std::string> viewsVerb(const Document& document, ViewVerbHost& host,
                              const std::vector<std::string>& args)
{
    ViewSet& views = host.views();
    const std::string action = args.empty() ? "LIST" : uppered(args[0]);
    const auto refuseWord = [](const std::string& word) {
        return makeError(ErrorCode::ParseFailure,
                         "VIEWS does not take '" + word + "': " + kViewsUsage, word);
    };

    if (action == "LIST") {
        if (args.size() > 1) {
            return refuseWord(args[1]);
        }
        std::string reply;
        for (const ViewState* view : views.views()) {
            reply += viewRecord(views, *view) + "\n";
        }
        return reply + "views=" + std::to_string(views.size()) +
               " linked=" + std::to_string(views.linkedViews().size());
    }
    if (action == "OPEN") {
        if (args.size() != 2) {
            return makeError(
                ErrorCode::ParseFailure,
                "VIEWS OPEN takes what the view shows: plan, 3d, section or elevation");
        }
        const auto kind = kindFrom(args[1]);
        if (!kind) {
            return makeError(ErrorCode::ParseFailure,
                             "VIEWS OPEN takes plan, 3d, section or elevation, not '" + args[1] +
                                 "'",
                             args[1]);
        }
        auto opened = host.open(*kind);
        if (!opened) {
            return opened.error();
        }
        const ViewState* view = views.find(*opened);
        if (view == nullptr) {
            return makeError(ErrorCode::Internal, "the window opened a view it does not list",
                             std::to_string(*opened));
        }
        return viewRecord(views, *view);
    }
    if (action == "ACTIVATE") {
        if (args.size() != 2) {
            return makeError(ErrorCode::ParseFailure, "VIEWS ACTIVATE takes one view id");
        }
        auto id = parseViewId(args[1]);
        if (!id) {
            return id.error();
        }
        if (views.find(*id) == nullptr) {
            return notOpen(*id);
        }
        if (auto status = host.activate(*id); !status) {
            return status.error();
        }
        return viewRecord(views, *views.find(*id));
    }
    if (action == "LINK") {
        std::size_t at = 1;
        auto ids = parseIds(args, at);
        if (!ids) {
            return ids.error();
        }
        std::optional<ViewId> to;
        if (at < args.size()) {
            // parseIds stopped at TO.
            if (at + 2 != args.size()) {
                return makeError(ErrorCode::ParseFailure,
                                 "TO takes the one view the others come to: VIEWS LINK "
                                 "<id>[,<id>...] [TO <id>]",
                                 args[at]);
            }
            auto leader = parseViewId(args[at + 1]);
            if (!leader) {
                return leader.error();
            }
            to = *leader;
        }
        auto change = views.link(*ids, to);
        if (!change) {
            return change.error();
        }
        host.changed(change->linked);
        return "leader=" + std::to_string(change->leader) + " linked=" + listed(change->linked) +
               " moved=" + listed(change->moved);
    }
    if (action == "UNLINK") {
        std::vector<ViewId> left;
        if (args.size() == 2 && uppered(args[1]) == "ALL") {
            left = views.unlinkAll();
        } else {
            std::size_t at = 1;
            auto ids = parseIds(args, at);
            if (!ids) {
                return ids.error();
            }
            if (at < args.size()) {
                return refuseWord(args[at]);
            }
            if (ids->empty()) {
                return makeError(ErrorCode::ParseFailure,
                                 "name the views to unlink: VIEWS UNLINK <id>[,<id>...] | ALL");
            }
            auto unlinked = views.unlink(*ids);
            if (!unlinked) {
                return unlinked.error();
            }
            left = std::move(unlinked).value();
        }
        host.changed(left);
        return "unlinked=" + listed(left) + " linked=" + listed(views.linkedViews());
    }
    if (action == "HIDE" || action == "SHOW" || action == "ISOLATE") {
        // The view's own filter: what its Layers button sets, by a line an
        // agent can type and a test can run - a design view hiding the
        // as-built layers beside an as-built view hiding the design ones.
        if (args.size() < 3) {
            return makeError(ErrorCode::ParseFailure,
                             "VIEWS " + action + " takes a view id and the layers: VIEWS " +
                                 action + (action == "ISOLATE" ? " <id> <layer>"
                                                               : " <id> <layer>[,<layer>...]"));
        }
        auto id = parseViewId(args[1]);
        if (!id) {
            return id.error();
        }
        ViewState* view = views.find(*id);
        if (view == nullptr) {
            return notOpen(*id);
        }
        if (action == "SHOW" && args.size() == 3 && uppered(args[2]) == "ALL") {
            view->layers.clear();
        } else {
            auto layers = parseLayers(document, args, 2);
            if (!layers) {
                return layers.error();
            }
            // Commas alone ("VIEWS HIDE 1 ,") name no layer: refused, where
            // the line changed nothing and replied as though it had worked.
            if (layers->empty()) {
                return makeError(ErrorCode::ParseFailure,
                                 "VIEWS " + action + " names no layer in '" + args[2] +
                                     "': VIEWS " + action +
                                     (action == "ISOLATE" ? " <id> <layer>"
                                                          : " <id> <layer>[,<layer>...]"),
                                 args[2]);
            }
            if (action == "ISOLATE") {
                if (layers->size() != 1) {
                    return makeError(ErrorCode::ParseFailure,
                                     "VIEWS ISOLATE takes one layer: the one the view is to show",
                                     args[2]);
                }
                // parseLayers found it, so isolate has a branch to keep.
                if (auto status =
                        view->layers.isolate(layers->front(), document.model().layers.names());
                    !status) {
                    return status.error();
                }
            } else {
                // A layer beneath one this view hides stays hidden whatever
                // is shown of it (LayerOverrides::show), so SHOW of it would
                // change nothing and say nothing: refused, naming what holds
                // it, before any layer of the line is shown.
                const auto holder = [&](const std::string& layer) -> std::optional<std::string> {
                    for (const std::string& ancestor : katana::entity::layerAncestors(layer)) {
                        if (view->layers.hidesDirectly(ancestor) &&
                            !std::ranges::contains(*layers, ancestor)) {
                            return ancestor;
                        }
                    }
                    return std::nullopt;
                };
                for (const std::string& layer : *layers) {
                    if (const auto held = action == "SHOW" ? holder(layer) : std::nullopt) {
                        return makeError(ErrorCode::CommandRejected,
                                         "view " + std::to_string(*id) + " hides '" + *held +
                                             "', which holds '" + layer + "': show '" + *held +
                                             "' (VIEWS SHOW " + std::to_string(*id) + " " +
                                             *held + ")",
                                         layer);
                    }
                }
                for (const std::string& layer : *layers) {
                    if (action == "HIDE") {
                        (void)view->layers.hide(layer);
                    } else {
                        (void)view->layers.show(layer);
                    }
                }
            }
        }
        host.settingsChanged(*id);
        return viewRecord(views, *view);
    }
    if (action == "SET") {
        // A view's own switches, as its Layers popup sets them. Every word
        // is read before any is applied, so a refused line changes nothing.
        constexpr const char* kSetUsage = "VIEWS SET <id> ghosts=on|off";
        if (args.size() < 3) {
            return makeError(ErrorCode::ParseFailure,
                             std::string("VIEWS SET takes a view id and what to set: ") +
                                 kSetUsage);
        }
        auto id = parseViewId(args[1]);
        if (!id) {
            return id.error();
        }
        ViewState* view = views.find(*id);
        if (view == nullptr) {
            return notOpen(*id);
        }
        bool ghosts = view->selectionGhosts;
        for (std::size_t at = 2; at < args.size(); ++at) {
            const std::string& word = args[at];
            const std::size_t equals = word.find('=');
            if (equals == std::string::npos || uppered(word.substr(0, equals)) != "GHOSTS") {
                return makeError(ErrorCode::ParseFailure,
                                 "VIEWS SET does not take '" + word + "': " + kSetUsage, word);
            }
            const std::string value = uppered(word.substr(equals + 1));
            if (value != "ON" && value != "OFF") {
                return makeError(ErrorCode::ParseFailure,
                                 "ghosts= is on or off, not '" + word.substr(equals + 1) + "'",
                                 word);
            }
            ghosts = value == "ON";
        }
        view->selectionGhosts = ghosts;
        host.settingsChanged(*id);
        return viewRecord(views, *view);
    }
    return refuseWord(args[0]);
}

// ---- ZOOM ---------------------------------------------------------------------------------

std::optional<Point2> parsePoint(std::string_view word)
{
    const std::size_t comma = word.find(',');
    if (comma == std::string_view::npos) {
        return std::nullopt;
    }
    const auto x = katana::core::parseFiniteDouble(word.substr(0, comma));
    const auto y = katana::core::parseFiniteDouble(word.substr(comma + 1));
    if (!x || !y) {
        return std::nullopt;
    }
    return Point2(*x, *y);
}

// "x0,y0,x1,y1" in one word, or "x0,y0 x1,y1" in two: the corners in either
// order. `at` is left after what was read.
Result<Box2> parseWindow(const std::vector<std::string>& args, std::size_t& at)
{
    const auto refuse = [&args, at] {
        return makeError(ErrorCode::ParseFailure,
                         "ZOOM WINDOW takes two corners: x0,y0,x1,y1",
                         at < args.size() ? args[at] : std::string());
    };
    if (at >= args.size()) {
        return refuse();
    }
    std::vector<double> numbers;
    std::string_view word = args[at];
    std::size_t start = 0;
    while (start <= word.size()) {
        const std::size_t comma = std::min(word.find(',', start), word.size());
        const auto value = katana::core::parseFiniteDouble(word.substr(start, comma - start));
        if (!value) {
            return refuse();
        }
        numbers.push_back(*value);
        start = comma + 1;
    }
    std::size_t used = 1;
    if (numbers.size() == 2 && at + 1 < args.size()) {
        const auto second = parsePoint(args[at + 1]);
        if (!second) {
            return refuse();
        }
        numbers.push_back(second->x);
        numbers.push_back(second->y);
        used = 2;
    }
    if (numbers.size() != 4) {
        return refuse();
    }
    const Point2 a(numbers[0], numbers[1]);
    const Point2 b(numbers[2], numbers[3]);
    if (a == b) {
        return makeError(ErrorCode::InvalidArgument, "a window needs two different corners",
                         args[at]);
    }
    at += used;
    return Box2(Point2(std::min(a.x, b.x), std::min(a.y, b.y)),
                Point2(std::max(a.x, b.x), std::max(a.y, b.y)));
}

// A zoom factor: finite and above 0.
Result<double> parseFactor(const std::string& word)
{
    const auto factor = katana::core::parseFiniteDouble(word);
    if (!factor || !(*factor > 0.0)) {
        return makeError(ErrorCode::ParseFailure,
                         "a zoom factor is a number above 0: above 1 is closer, below 1 farther",
                         word);
    }
    return *factor;
}

// A factor as AutoCAD writes one relative to the view, "2X" or "0.5x", is a
// bare factor here, which is always relative to the view: the number, else
// the word as it is. ("2XP", relative to paper space, stays a word ZOOM
// refuses: a plan view has no paper space.)
std::string timesView(const std::string& word)
{
    if (word.size() > 1 && (word.back() == 'X' || word.back() == 'x')) {
        return word.substr(0, word.size() - 1);
    }
    return word;
}

// The word the refusal of a kind names the request by.
std::string requestWord(const ZoomRequest& request)
{
    switch (request.kind) {
    case ZoomRequest::Kind::Extents:
        return "EXTENTS";
    case ZoomRequest::Kind::In:
        return "IN";
    case ZoomRequest::Kind::Out:
        return "OUT";
    case ZoomRequest::Kind::Factor:
        return "by a factor";
    case ZoomRequest::Kind::Window:
        return "WINDOW";
    case ZoomRequest::Kind::Centre:
        return "CENTRE";
    case ZoomRequest::Kind::Scope:
        return "on a scope";
    }
    return "";
}

Result<std::string> zoomVerb(const Document& document, ViewVerbHost& host,
                             std::vector<std::string> args, const ScopeViewProvider& scopeViews)
{
    ViewSet& views = host.views();
    ZoomRequest request;

    // view= first, wherever it stands, so nothing after reads it as a word of
    // its own - after WHERE it would be taken for a condition and refused.
    std::optional<ViewId> named;
    for (auto word = args.begin(); word != args.end();) {
        if (word->size() >= 5 && katana::core::equalsIgnoringCase(word->substr(0, 5), "view=")) {
            if (named) {
                return makeError(ErrorCode::ParseFailure, "give view= once", *word);
            }
            auto id = parseViewId(std::string_view(*word).substr(5));
            if (!id) {
                return id.error();
            }
            named = *id;
            word = args.erase(word);
        } else {
            ++word;
        }
    }
    if (named) {
        if (views.find(*named) == nullptr) {
            return notOpen(*named);
        }
        request.view = *named;
    } else {
        request.view = views.activeId();
        if (request.view == kNoView) {
            return makeError(ErrorCode::InvalidState,
                             "no view is open to zoom: VIEWS OPEN plan opens one");
        }
    }

    const auto refuseWord = [](const std::string& word) {
        return makeError(ErrorCode::ParseFailure,
                         "ZOOM does not take '" + word + "': " + kZoomUsage, word);
    };
    std::size_t at = 0;
    // What a scope took, when the line gave one.
    std::optional<ScopeMatch> scope;
    // ZOOM A and ZOOM ALL alone are Zoom All, the extents - every CAD
    // program's, and the zoom line a script most often holds; on the old
    // window it zoomed the extents whatever followed ZOOM, so a refusal here
    // stopped a script that ran to the end there. ALL is the shared scope
    // word for the drawing too, and is that with a filter after it (ZOOM ALL
    // WHERE ...), which frames the entities it matches.
    const bool zoomAll =
        args.size() == 1 && (uppered(args[0]) == "A" || uppered(args[0]) == "ALL");
    if (!args.empty() && isScopeWord(args[0]) && !zoomAll) {
        // The one parser and the one matcher every verb on drawing data uses.
        // Only with a scope word: ZOOM alone is EXTENTS, where no scope word
        // elsewhere means the selection.
        auto words = parseScopeWords(args, at);
        if (!words) {
            return words.error();
        }
        if (at < args.size()) {
            return refuseWord(args[at]);
        }
        auto matched = matchScope(document, *words, scopeViews);
        if (!matched) {
            return matched.error();
        }
        scope = std::move(matched).value();
        request.kind = ZoomRequest::Kind::Scope;
        request.window = extentOf(document.model(), scope->matched);
        request.ids = scope->matched;
    } else if (!args.empty()) {
        const std::string word = uppered(args[0]);
        at = 1;
        if (word == "EXTENTS" || word == "E" || zoomAll) {
            request.kind = ZoomRequest::Kind::Extents;
        } else if (word == "IN" || word == "OUT") {
            request.kind = word == "IN" ? ZoomRequest::Kind::In : ZoomRequest::Kind::Out;
            if (at < args.size() && katana::core::parseFiniteDouble(args[at])) {
                auto factor = parseFactor(args[at]);
                if (!factor) {
                    return factor.error();
                }
                request.factor = *factor;
                ++at;
            }
        } else if (word == "WINDOW" || word == "W") {
            request.kind = ZoomRequest::Kind::Window;
            auto box = parseWindow(args, at);
            if (!box) {
                return box.error();
            }
            request.window = *box;
        } else if (word == "CENTRE" || word == "CENTER" || word == "C") {
            request.kind = ZoomRequest::Kind::Centre;
            const auto centre = at < args.size() ? parsePoint(args[at]) : std::nullopt;
            if (!centre) {
                return makeError(ErrorCode::ParseFailure,
                                 "ZOOM CENTRE takes the point to centre on: x,y",
                                 at < args.size() ? args[at] : std::string());
            }
            request.centre = *centre;
            ++at;
            if (at < args.size() && uppered(args[at]) == "SCALE") {
                const auto scale =
                    at + 1 < args.size() ? katana::core::parseFiniteDouble(args[at + 1])
                                         : std::nullopt;
                if (!scale || *scale < ViewTransform::kMinimumScale ||
                    *scale > ViewTransform::kMaximumScale) {
                    // The limits a plan view clamps to, read from there so the
                    // message cannot drift from them.
                    return makeError(ErrorCode::InvalidArgument,
                                     "SCALE is pixels per unit from " +
                                         real(ViewTransform::kMinimumScale) + " to " +
                                         real(ViewTransform::kMaximumScale),
                                     at + 1 < args.size() ? args[at + 1] : args[at]);
                }
                request.scale = *scale;
                at += 2;
            }
        } else if (const std::string number = timesView(args[0]);
                   katana::core::parseFiniteDouble(number)) {
            request.kind = ZoomRequest::Kind::Factor;
            auto factor = parseFactor(number);
            if (!factor) {
                return factor.error();
            }
            request.factor = *factor;
        } else {
            return refuseWord(args[0]);
        }
        if (at < args.size()) {
            return refuseWord(args[at]);
        }
    }

    const ViewState& view = *views.find(request.view);
    if (!zoomTakes(view.kind, request.kind)) {
        // The kinds that take it, read from zoomTakes itself so the message
        // cannot drift from the rule. A plan view takes every request, so
        // the list is never empty and "a plan" leads it.
        std::vector<std::string> takers;
        for (const ViewKind kind :
             {ViewKind::Plan, ViewKind::Model3D, ViewKind::Elevation, ViewKind::Section}) {
            if (zoomTakes(kind, request.kind)) {
                takers.push_back(kind == ViewKind::Model3D ? "3D" : kindWord(kind));
            }
        }
        std::string which = "a " + takers.front();
        for (std::size_t i = 1; i < takers.size(); ++i) {
            which += (i + 1 == takers.size() ? " or " : ", ") + takers[i];
        }
        const char* does = request.kind == ZoomRequest::Kind::Window ||
                                   request.kind == ZoomRequest::Kind::Scope
                               ? "frames"
                           : request.kind == ZoomRequest::Kind::Centre ? "centres"
                                                                       : "zooms";
        return makeError(ErrorCode::InvalidArgument,
                         "ZOOM " + requestWord(request) + " " + does + " " + which +
                             " view: view " + std::to_string(view.id) + " is " +
                             toString(view.kind) + " (ZOOM EXTENTS frames any view)",
                         std::to_string(view.id));
    }

    std::string reply;
    if (scope) {
        reply = scopeRecord(*scope);
        // Taking nothing is an answer, not a refusal: nothing to frame, and
        // no view moved.
        if (request.window.empty()) {
            return reply;
        }
    }
    auto moved = host.zoom(request);
    if (!moved) {
        return moved.error();
    }
    for (const ViewId id : *moved) {
        const ViewState* each = views.find(id);
        if (each == nullptr) {
            continue;
        }
        if (!reply.empty()) {
            reply += "\n";
        }
        reply += "view=" + std::to_string(id) + " kind=" + kindWord(each->kind);
        if (id != request.view) {
            reply += " followed=" + std::to_string(request.view);
        }
        reply += placeOf(*each, false);
    }
    return reply;
}

} // namespace

bool zoomTakes(ViewKind kind, ZoomRequest::Kind request)
{
    switch (request) {
    case ZoomRequest::Kind::Extents:
    case ZoomRequest::Kind::In:
    case ZoomRequest::Kind::Out:
    case ZoomRequest::Kind::Factor:
        return true;
    case ZoomRequest::Kind::Scope:
        return kind != ViewKind::Section;
    case ZoomRequest::Kind::Window:
    case ZoomRequest::Kind::Centre:
        return kind == ViewKind::Plan;
    }
    return false;
}

bool applyPlanZoom(ViewTransform& view, const ZoomRequest& request)
{
    // About the centre pixel, where screenToWorld gives the centre back
    // exactly: ViewTransform::zoomAt then leaves the centre where it was and
    // clamps the scale as the wheel's zoom does.
    const Point2 middle(0.5 * view.widthPixels, 0.5 * view.heightPixels);
    switch (request.kind) {
    case ZoomRequest::Kind::In:
    case ZoomRequest::Kind::Factor:
        view.zoomAt(middle, request.factor);
        return true;
    case ZoomRequest::Kind::Out:
        view.zoomAt(middle, 1.0 / request.factor);
        return true;
    case ZoomRequest::Kind::Centre:
        view.center = request.centre;
        if (request.scale) {
            view.scale = *request.scale;
        }
        return true;
    case ZoomRequest::Kind::Extents:
    case ZoomRequest::Kind::Window:
    case ZoomRequest::Kind::Scope:
        return false;
    }
    return false;
}

bool isViewVerb(std::string_view verb) { return verb == "VIEWS" || verb == "ZOOM"; }

Result<std::string> runViewVerb(const Document& document, std::string_view verb,
                                const std::vector<std::string>& args,
                                const ViewHostProvider& provider,
                                const ScopeViewProvider& scopeViews)
{
    ViewVerbHost* host = provider ? provider() : nullptr;
    if (host == nullptr) {
        return makeError(ErrorCode::Unsupported,
                         std::string(verb) +
                             " is the desktop window's: it acts on the window's views, which "
                             "katana_cli and katana_mcp do not have; run it on the window's "
                             "command line or with katana --command");
    }
    if (verb == "VIEWS") {
        return viewsVerb(document, *host, args);
    }
    return zoomVerb(document, *host, args, scopeViews);
}

std::string viewRecord(const ViewSet& views, const ViewState& view)
{
    return "view=" + std::to_string(view.id) + " kind=" + kindWord(view.kind) +
           " title=" + recordValue(ViewSet::title(view)) +
           " active=" + (views.activeId() == view.id ? "yes" : "no") + placeOf(view, true) +
           " ghosts=" + (view.selectionGhosts ? "on" : "off") + hiddenOf(view);
}

} // namespace katana::cad
