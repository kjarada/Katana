#pragma once

// The road model's 3D objects - horizontal BIM: every pavement course, kerb,
// footpath, verge, batter, barrier, sign, light pole, culvert and wall of a
// road (docs/road_design.md, docs/road_objects.md).
//
// An object is MADE, every time, from the drawing's definitions: a corridor
// and the road template it sweeps along an alignment and its profile
// (cad/road/), or an object placed along an alignment or a line
// (cad/road/road_objects.hpp). The definitions are the data - saved with the
// project, edited by verbs as one undo step - and these objects are what the
// 3D view, the plan view, the quantities and the IFC export read. None of
// them is stored, so none can disagree with the definition it came from.
//
// A definition that cannot be built (an alignment with no profile, a station
// off the alignment, a template with no component) builds what it can and
// says what it could not in `problems`, a list an agent reads - the views
// still draw the rest.

#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/geometry/mesh.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {
class Document;
}

namespace katana::cad::bim {

// A measured quantity of an object, named as IFC's base quantities are
// (Length, Width, Depth, Height, GrossArea, NetArea, GrossVolume, NetVolume,
// Count, Weight) so the IFC export writes them as they are.
struct Quantity {
    std::string name;
    double value = 0.0;
    std::string unit; // "m", "m2", "m3", "" for a count

    friend bool operator==(const Quantity&, const Quantity&) = default;
};

struct BimObject {
    // Stable: the same definition gives the same id on every run and every
    // machine ("corridor/Main/course/AC14/left/3"), since the IFC export
    // derives the GlobalId from it and a model handed over twice must name
    // the same things the same way.
    std::string id;
    std::string name;
    // What it is, in Katana's words: "pavement", "kerb", "footpath",
    // "verge", "batter", "earthworks", "barrier", "sign", "light", "culvert",
    // "wall", "marking", "guidepost", "fence".
    std::string kind;
    // The facility it belongs to - a corridor's name, which the IFC export
    // writes as an IfcRoad - and the part of it, as IfcRoadPart's predefined
    // type: CARRIAGEWAY, TRAFFICLANE, SHOULDER, SIDEWALK, ROADSIDE,
    // CENTRALRESERVE, ROADSEGMENT. Empty: an object of the site.
    std::string facility;
    std::string part;
    // How IFC 4.3 names it: the class (IfcCourse, IfcPavement, IfcKerb,
    // IfcRailing, IfcSign, IfcEarthworksFill, IfcEarthworksCut, ...) and its
    // predefined type, both as the schema spells them.
    std::string ifcClass;
    std::string predefinedType;
    std::string material;
    katana::entity::Color colour{};
    // In world coordinates. Closed where the object is a solid.
    katana::geometry::TriangleMesh mesh;
    // What the plan view draws of it: its outline, or its line for a thin
    // object (a barrier, a line marking). Empty: the plan view draws
    // the mesh's plan hull.
    std::vector<katana::geometry::Polyline2> plan;
    std::vector<Quantity> quantities;
    // Further attributes, in order: the template component, the station
    // range, the course thickness, the sign code...
    std::vector<std::pair<std::string, std::string>> properties;
    // The drawing layer it is shown on; its visibility follows the layer's.
    std::string layer;

    friend bool operator==(const BimObject&, const BimObject&) = default;
};

struct BimProblem {
    std::string source;  // the definition: "corridor Main", "object sign-3"
    std::string message; // what could not be built, and why

    friend bool operator==(const BimProblem&, const BimProblem&) = default;
};

struct BimModel {
    std::vector<BimObject> objects;
    std::vector<BimProblem> problems;

    friend bool operator==(const BimModel&, const BimModel&) = default;
};

// The objects of every corridor of the drawing, corridor by corridor in the
// order they were defined (src/katana_cad/road/corridor_objects.cpp).
[[nodiscard]] BimModel corridorObjects(const Document& document);
// The objects placed along an alignment or a line, in the order they were
// placed (src/katana_cad/road/road_objects.cpp).
[[nodiscard]] BimModel placedObjects(const Document& document);
// Both, corridors first, made once per change of what they are made from -
// the definitions, the alignments, the surfaces - and shared, so a view can
// call it every frame (src/katana_cad/bim/bim_object.cpp).
[[nodiscard]] std::shared_ptr<const BimModel> bimModel(const Document& document);

} // namespace katana::cad::bim
