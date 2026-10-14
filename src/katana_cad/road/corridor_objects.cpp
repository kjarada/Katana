// bim::corridorObjects (include/katana/cad/bim/bim_object.hpp): the road
// model's objects of every corridor of the drawing, made from the corridors
// and the templates each time (cad/road/road_section.hpp).

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_section.hpp"
#include "katana/cad/road/road_template.hpp"

namespace katana::cad::bim {

BimModel corridorObjects(const Document& document)
{
    BimModel model;
    // Definitions that cannot be read build nothing, and say so: a project
    // a newer Katana wrote, or one damaged by hand.
    if (const auto status = document.roadTemplatesStatus(); !status) {
        model.problems.push_back(BimProblem{"road templates", status.error().describe()});
    }
    if (const auto status = document.corridorsStatus(); !status) {
        model.problems.push_back(BimProblem{"corridors", status.error().describe()});
    }
    for (const road::Corridor& corridor : document.corridors().corridors) {
        BimModel made = road::corridorSolids(road::buildCorridor(document, corridor));
        model.objects.insert(model.objects.end(), std::make_move_iterator(made.objects.begin()),
                             std::make_move_iterator(made.objects.end()));
        model.problems.insert(model.problems.end(), made.problems.begin(), made.problems.end());
    }
    return model;
}

} // namespace katana::cad::bim
