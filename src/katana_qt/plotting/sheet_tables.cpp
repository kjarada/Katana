#include "sheet_tables.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;

plotting::SheetSet resolvedSheetSet(const plotting::SheetSet& set, const SheetSource& source,
                                    SheetPaintCache* cache)
{
    SheetPaintCache own;
    SheetPaintCache& sections = cache != nullptr ? *cache : own;
    plotting::SheetSet drawn = set;
    for (std::size_t s = 0; s < drawn.sheets.size(); ++s) {
        for (plotting::Viewport& viewport : drawn.sheets[s].viewports) {
            const bool planLike = viewport.kind == plotting::ViewportKind::Plan ||
                                  viewport.kind == plotting::ViewportKind::KeyPlan;
            const bool section = viewport.kind == plotting::ViewportKind::LongSection ||
                                 viewport.kind == plotting::ViewportKind::CrossSections;
            if (planLike && (viewport.autoScale || viewport.autoCentre)) {
                // With the set and the sheet, as the painter resolves it: an
                // automatic key plan fits the sheets' outlines, not the drawing.
                const ResolvedViewport at = resolvePlanViewport(viewport, source, set, s);
                viewport.scale = at.scale;
                viewport.centre = at.centre;
                viewport.autoScale = false;
                viewport.autoCentre = false;
            } else if (section && viewport.autoScale) {
                // The scale and exaggeration the section is fitted to (section_fit.hpp),
                // so a register row says "H 1:1000 V 1:100" as its title block does.
                const plotting::SectionFit fit = resolveSectionViewport(viewport, source, sections);
                viewport.scale = fit.scale;
                viewport.verticalExaggeration = fit.exaggeration;
                viewport.autoScale = false;
            }
        }
    }
    return drawn;
}

std::vector<plotting::RegisterRow> drawnRegister(const plotting::SheetSet& set,
                                                 const SheetSource& source)
{
    return plotting::drawingRegister(resolvedSheetSet(set, source));
}

} // namespace katana::qt
