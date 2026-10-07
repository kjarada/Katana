#pragma once

// What a colour NAME means in a drawing.
//
// A survey code rule, the symbol it gives and a pen inside a definition all
// name their colour. ONE resolver answers for a Document: its customisation's
// own table, then the standard names (entity::resolveColour). The standard
// names were once a table cad could not see, so each front end handed cad a
// function of its own - and a customisation's names ("sui test water")
// resolved nowhere at all.

#include <functional>
#include <optional>
#include <string_view>

#include "katana/cad/document.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad {

// The colour `name` means in `document`; nullopt for a name neither its
// customisation nor the standard names know, and the caller then leaves the
// colour alone.
[[nodiscard]] std::optional<katana::entity::Color> resolveColour(const Document& document,
                                                                 std::string_view name);

// resolveColour as a function, in the shape SurveyCodingOptions::colourOf and
// ColourLookup (code_table.hpp) take. It asks `document` at each call, so it
// follows a colour table installed after it was made - and it must not outlive
// the Document.
//
// A caller that passes NO lookup still gets no colours: this is offered, not
// applied behind anyone's back.
[[nodiscard]] std::function<std::optional<katana::entity::Color>(std::string_view)>
colourLookup(const Document& document);

} // namespace katana::cad
