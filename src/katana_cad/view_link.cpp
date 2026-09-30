#include "katana/cad/view_link.hpp"

namespace katana::cad {

bool linkable(ViewKind kind) { return kind == ViewKind::Plan; }

bool followView(const ViewState& from, ViewState& to)
{
    if (from.kind != ViewKind::Plan || to.kind != ViewKind::Plan) {
        return false;
    }
    to.plan.center = from.plan.center;
    to.plan.scale = from.plan.scale;
    return true;
}

} // namespace katana::cad
