#pragma once

// The scope and filter words every verb that reads or changes drawing data
// takes (docs/cad.md, "Scope and filter: one grammar for every verb on
// drawing data"): the owner's request of 2026-09-26, "i want the utility tools to act on data on view,
// layer/s, elements, filtered elements, like global change ... all tools need
// to have the ability to act that way".
//
// ONE grammar, read by ONE parser, resolved by the one matcher Global Modify
// already has (cad::matchEntities, global_modify.hpp), so MODIFY, the UTILITY
// verbs and every verb after them take a scope the same way:
//
//   SELECTION | DRAWING | VIEW [<view id>] | AREA x0,y0,x1,y1 | LAYERS a,b[,c] [ONLY]
//   then [WHERE key=value ...]
//
// Words are case-insensitive; SEL, ALL and LAYER are accepted for SELECTION,
// DRAWING and LAYERS, as MODIFY accepted them. No scope word is the selection.
// The WHERE keys are MODIFY's: TYPE=point,line LAYER=pat[,pat] STYLE=pat|ByLayer
// COLOUR=#RRGGBB|ByLayer PROP=key[:pat] TEXT=pat DRAWN, with '*' and '?'
// wildcards (matchesPattern).
//
// It is read in two steps so each can be tested alone and a front end can
// build the words without a document:
//
//   parseScopeWords   words -> ScopeWords: plain data, nothing looked up.
//   resolveScope      ScopeWords -> a ModifyScope and ModifyFilter, asking
//                     the window for a VIEW.
//
// VIEW needs the window: the Document holds no views, so the front end that
// has them answers for the active plan view, or the view with that id, with
// its own hidden layers and its visible area (ScopeViewProvider). Headless
// there is no view, and VIEW is refused naming AREA, which is the same scope
// with the window typed in: a view that hides nothing of its own, looking at
// that box. That is why AREA is a View scope with an area and no view layers
// rather than a kind of its own - matchEntities already reads a View scope
// exactly so, and a second reading would be a second implementation.

#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/global_modify.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// Which scope word was given.
enum class ScopeSource { Selection, Drawing, View, Area, Layers };

// The scope and filter words as read: plain data, nothing looked up. What a
// front end fills to say a scope (formatScopeWords) and what the parser gives.
struct ScopeWords {
    ScopeSource source = ScopeSource::Selection;
    // VIEW <id>: that view (a cad::ViewId); empty: the active plan view.
    std::optional<std::uint32_t> view{};
    // AREA: min before max, whichever corners were typed first.
    katana::geometry::Box2 area{};
    // LAYERS: the layer paths, each of which must exist, and whether each
    // takes the layers beneath it (no ONLY).
    std::vector<std::string> layers{};
    bool sublayers = true;
    // WHERE.
    ModifyFilter filter{};
};

// True for a word that begins a scope - SELECTION, SEL, DRAWING, ALL, VIEW,
// AREA, LAYERS, LAYER - or its filter, WHERE; any case. What a verb that also
// takes a file asks of its first word: a file whose name is one of these is
// given with its directory ("./drawing").
[[nodiscard]] bool isScopeWord(std::string_view word);

// Reads the scope and filter words of `words` from `at` and leaves `at` at
// the first word that is neither, where the verb's own words begin. A scope
// word is read only before WHERE; after WHERE, a word holding '=' or DRAWN is
// a condition and anything else ends the filter.
//
// Refused, naming the word: two scope words; LAYERS with no layer after it;
// ONLY other than after a LAYERS list; AREA without four finite numbers; VIEW
// with an id of 0; a WHERE word with '=' whose key is not a WHERE key, or
// whose value does not read (TYPE=blob, COLOUR=red).
[[nodiscard]] katana::core::Result<ScopeWords>
parseScopeWords(const std::vector<std::string>& words, std::size_t& at);

// The words that say `words` back, as a line takes them: parseScopeWords
// reads the result as `words`. The scope word is always written, so the
// result can begin a UTILITY line; a word with a blank in it is quoted.
// InvalidArgument for what a line cannot say: a double quote anywhere (the
// command line has no way to type one inside a word), a comma in a layer name
// or pattern (it separates the list), an empty layer list.
[[nodiscard]] katana::core::Result<std::string> formatScopeWords(const ScopeWords& words);

// "#RRGGBB" or ByLayer (any case), as an entity's or a style's colour says it:
// the inner nullopt is ByLayer. MODIFY's SET COLOUR= reads it too.
[[nodiscard]] katana::core::Result<std::optional<katana::entity::Color>>
parseColourOrByLayer(std::string_view text);

// ---- the window's side ---------------------------------------------------------------------

// What the window knows of a plan view that the Document does not.
struct ScopeView {
    std::uint32_t id = 0; // its cad::ViewId, for the reply
    // The layers it hides of its own (ViewState::layers).
    LayerOverrides layers{};
    // What it shows: ViewTransform::visibleWorldBounds. Empty: everything it
    // draws, wherever it lies.
    std::optional<katana::geometry::Box2> area{};
};

// Answers VIEW: the active plan view when `id` is empty, else the view with
// that id. A front end fails with NotFound naming an id that is no open plan
// view, and InvalidState when no plan view is open.
using ScopeViewProvider =
    std::function<katana::core::Result<ScopeView>(std::optional<std::uint32_t> id)>;

// The words resolved to what matchEntities takes. It holds the view's layers,
// which `scope.view` points at, so a copy or a move keeps `scope` good.
struct ResolvedScope {
    ScopeWords words{};
    ModifyScope scope{};
    ModifyFilter filter{};
    // VIEW: the view it resolved to.
    std::optional<std::uint32_t> view{};
    std::shared_ptr<const LayerOverrides> viewLayers{};
};

// VIEW asks `views`; with no provider (headless) it is refused, naming AREA.
// Nothing else is looked up here: a layer the drawing lacks is matchEntities'
// refusal.
[[nodiscard]] katana::core::Result<ResolvedScope> resolveScope(const ScopeWords& words,
                                                               const ScopeViewProvider& views);

// What a scope took, for a reply.
struct ScopeMatch {
    ResolvedScope resolved{};
    // Ascending, as matchEntities gives them.
    std::vector<katana::entity::EntityId> matched{};
};

// resolveScope, then matchEntities. Taking nothing is not a failure: a scope
// that matches nothing is reported, as Global Modify's preview reports it.
[[nodiscard]] katana::core::Result<ScopeMatch>
matchScope(const Document& document, const ScopeWords& words, const ScopeViewProvider& views);

// The record a reply carries to say what the scope took, key=value:
//   scope=drawing matched=21
//   scope=layers layers=utilities/water sublayers=no where="PROP=utility.line:W1" matched=6
//   scope=view view=3 matched=40      scope=area area=0,0,10,10 matched=2
// A verb adds its own keys after it.
[[nodiscard]] std::string scopeRecord(const ScopeMatch& match);

// A value in a reply record: as it is when it is one plain word, else in
// double quotes with '"' and '\' escaped and a line break made a blank, so a
// record still splits on blanks. The UTILITY records use it too.
[[nodiscard]] std::string recordValue(std::string_view value);

} // namespace katana::cad
