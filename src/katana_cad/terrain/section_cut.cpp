#include "katana/cad/terrain/section_cut.hpp"

#include <algorithm>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;

double drawingSectionInterval(double length)
{
    return std::max(length / kSectionStations, kLeastSectionInterval);
}

katana::core::Result<DrawingSection> cutDrawingSection(const Document& document,
                                                       const katana::geometry::Polyline2& alignment,
                                                       const SectionDesign* design)
{
    std::vector<SectionSurfaceInput> inputs;
    for (const katana::terrain::NamedSurface& named : document.surfaces().all()) {
        if (named.visible && named.surface) {
            inputs.push_back(SectionSurfaceInput{named.name, named.surface.get()});
        }
    }
    if (inputs.empty()) {
        return document.surfaces().empty()
                   ? makeError(ErrorCode::NotFound, "the drawing has no surface to cut a section of")
                   : makeError(ErrorCode::InvalidState, "every surface of the drawing is hidden");
    }
    SectionOptions options;
    options.interval = drawingSectionInterval(alignment.length());
    auto section = extractSection(alignment, inputs, &document.model(), options);
    if (!section) {
        return section.error();
    }
    if (design != nullptr) {
        if (auto status = appendDesignProfile(*section, design->profile, design->name); !status) {
            return status.error();
        }
    }
    return DrawingSection{std::move(*section), options.interval};
}

} // namespace katana::cad
