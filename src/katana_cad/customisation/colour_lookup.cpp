#include "katana/cad/colour_lookup.hpp"

#include "katana/entity/colour_names.hpp"

namespace katana::cad {

std::optional<katana::entity::Color> resolveColour(const Document& document, std::string_view name)
{
    return katana::entity::resolveColour(document.customisationState().colours, name);
}

std::function<std::optional<katana::entity::Color>(std::string_view)>
colourLookup(const Document& document)
{
    return [&document](std::string_view name) { return resolveColour(document, name); };
}

} // namespace katana::cad
