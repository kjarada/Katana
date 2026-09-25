#include "sheet_tables.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;

plotting::SheetSet resolvedSheetSet(const plotting::SheetSet& set, const SheetSource& source)
{
    plotting::SheetSet drawn = set;
    for (plotting::Sheet& sheet : drawn.sheets) {
        for (plotting::Viewport& viewport : sheet.viewports) {
            const bool planLike = viewport.kind == plotting::ViewportKind::Plan ||
                                  viewport.kind == plotting::ViewportKind::KeyPlan;
            if (!planLike || (!viewport.autoScale && !viewport.autoCentre)) {
                continue;
            }
            const ResolvedViewport at = resolvePlanViewport(viewport, source);
            viewport.scale = at.scale;
            viewport.centre = at.centre;
            viewport.autoScale = false;
            viewport.autoCentre = false;
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
