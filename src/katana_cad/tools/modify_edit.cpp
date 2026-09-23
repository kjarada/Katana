// Trim, Extend, Offset, Fillet, Chamfer, Break, Break at Point, Join, Explode
// (see families.hpp): the Modify menu's Edit group.
//
// Every tool here builds its whole operation in an EditSession
// (modify_edit_support.hpp) and hands the document ONE command when it
// finishes, so one undo takes back everything the tool did - all the picks of
// a Trim, all the corners of a Multiple fillet. The session is also why these
// tools do not return trimEntity, filletEntities and their like directly: a
// second pick can land on a piece the first one made, which has no id until a
// command runs, and a fillet must keep the side of each line the user picked,
// which filletEntities does not know. The geometry is the same functions those
// commands use (geometry/editing.hpp).

#include <memory>
#include <utility>

#include "families.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools {

void addModifyEditTools(ToolCatalog& catalog, const Report& report)
{
    using namespace modify_edit;
    // One set of remembered values for the program, as AutoCAD keeps
    // FILLETRAD and OFFSETDIST: the catalogue is built once, so the factories
    // share this object for as long as it runs.
    const auto defaults = std::make_shared<EditDefaults>();

    const auto add = [&](std::string id, std::string name, int order,
                         std::vector<std::string> aliases, std::string tip,
                         std::function<std::unique_ptr<InteractiveTool>(const ToolContext&)> make) {
        ToolInfo info;
        info.id = std::move(id);
        info.name = std::move(name);
        info.category = "Modify";
        info.group = "Edit";
        info.order = order;
        info.aliases = std::move(aliases);
        info.tip = std::move(tip);
        info.make = std::move(make);
        report(catalog.add(std::move(info)));
    };

    add("modify.trim", "Trim", 10, {"TRIM", "TR"},
        "Cuts objects back to cutting edges: pick the edges and press Enter (or Enter at once "
        "for every object), then pick each part to remove.",
        [](const ToolContext& context) { return makeTrimTool(context); });
    add("modify.extend", "Extend", 20, {"EXTEND", "EX"},
        "Lengthens lines, arcs and open polylines to boundary edges: pick the boundaries and "
        "press Enter (or Enter at once for every object), then pick near each end to extend.",
        [](const ToolContext& context) { return makeExtendTool(context); });
}

} // namespace katana::cad::tools
