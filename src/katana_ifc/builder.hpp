#pragma once

// What every part of the export shares: the file, the project frame (local
// origin, placements, representation contexts), property sets, relationships
// and the tallies of the report. Internal to katana_ifc.
//
// The parts - alignments (alignment.cpp), services (utilities.cpp), drawing
// entities (drawing.cpp) and surfaces (surfaces.cpp) - each add their objects
// through it; the relationships that gather objects from all of them
// (containment in the site, presentation layers, classification) are written
// once, last, by finish().

#include <array>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/ifc/export.hpp"
#include "katana/math/vec2.hpp"
#include "katana/math/vec3.hpp"
#include "step_file.hpp"

namespace katana::ifc::detail {

using katana::math::Vec2;
using katana::math::Vec3;

// One property of a property set, its value already encoded as a typed IFC
// value (IFCLABEL('x')).
struct Property {
    std::string name;
    std::string value;         // encoded; empty for no value
    std::string specification; // what the property means; empty for none
    // IfcPropertyEnumeratedValue instead of IfcPropertySingleValue: `value`
    // is the one chosen, `enumeration` the IfcPropertyEnumeration listing the
    // choices (0 for none), as the standard property sets and a delivery
    // schema's domains have them.
    bool enumerated = false;
    Id enumeration = 0;
};

// Properties in the order they are added. A value that is absent - an empty
// text, nullopt - adds nothing: a property with no value says less than no
// property, and IFC's own property sets are read that way.
class PropertyList {
  public:
    // IfcLabel, or IfcText when longer than an IfcLabel's 255 characters.
    PropertyList& label(std::string_view name, std::string_view value,
                        std::string_view specification = {});
    PropertyList& text(std::string_view name, std::string_view value);
    PropertyList& identifier(std::string_view name, std::string_view value);
    PropertyList& length(std::string_view name, std::optional<double> value);
    // IfcPositiveLengthMeasure: written only when the value is above zero.
    PropertyList& positiveLength(std::string_view name, std::optional<double> value);
    PropertyList& ratio(std::string_view name, std::optional<double> value);
    PropertyList& positiveRatio(std::string_view name, std::optional<double> value);
    PropertyList& real(std::string_view name, std::optional<double> value);
    PropertyList& integer(std::string_view name, std::optional<long long> value);
    PropertyList& boolean(std::string_view name, std::optional<bool> value);
    // An IfcDate, "YYYY-MM-DD".
    PropertyList& date(std::string_view name, std::string_view isoDate);
    PropertyList& enumerated(std::string_view name, std::string_view label, Id enumeration = 0,
                             std::string_view specification = {});
    PropertyList& add(Property property);

    [[nodiscard]] bool empty() const { return items_.empty(); }
    [[nodiscard]] const std::vector<Property>& items() const { return items_; }

  private:
    std::vector<Property> items_;
};

// An entity's own properties (the attribute manager's), each by its name and
// typed as it is typed in Katana.
void addEntityProperties(PropertyList& list, const entity::PropertyMap& properties);

// The frame the whole file is written in.
struct Frame {
    Vec3 origin{};              // the project coordinates of the file's (0, 0, 0)
    bool georeferenced = false; // an IfcMapConversion records `origin`
};

class Builder {
  public:
    Builder(const ExportOptions& options, Frame frame, IfcExport& report);

    [[nodiscard]] StepFile& file() { return file_; }
    [[nodiscard]] const ExportOptions& options() const { return options_; }
    [[nodiscard]] IfcExport& report() { return report_; }
    void warn(std::string message) { report_.warnings.push_back(std::move(message)); }
    // Accounts for `count` more objects of `source` written as `ifcClass` (in
    // `system`), for the reason `why` (IfcExport::tally); an empty class
    // entity counts objects that were not written. finish() files the
    // account in the report, in order.
    void tally(std::string_view source, const IfcClass& ifcClass, std::string_view system,
               std::string_view why, std::size_t count = 1);

    // A GlobalId for `key`, unique in the file even if two keys hash alike
    // (they never have; the check costs nothing).
    [[nodiscard]] std::string guid(std::string_view key);

    // ---- geometry, in the file's local coordinates --------------------------
    [[nodiscard]] Vec2 local(Vec2 world) const;
    [[nodiscard]] Vec3 local(Vec3 world) const;
    Id point(Vec2 world);      // IfcCartesianPoint, 2D
    Id point(Vec3 world);      // IfcCartesianPoint, 3D
    Id localPoint(Vec2 local); // already local
    Id direction(double x, double y);
    Id direction(double x, double y, double z);
    Id origin2();                             // (0, 0), shared
    Id placement2(Id location, double angle); // IfcAxis2Placement2D
    Id identity3();                           // IfcAxis2Placement3D at (0, 0, 0), shared
    Id unitLine();                            // IfcLine through (0, 0) along +x, shared: every
                                              // straight IfcCurveSegment's parent
    Id polyline(const std::vector<Vec2>& world, bool closed);
    Id polyline(const std::vector<Vec3>& world, bool closed);

    // ---- the project frame ----------------------------------------------------
    [[nodiscard]] Id project() const { return project_; }
    [[nodiscard]] Id site() const { return site_; }
    [[nodiscard]] Id sitePlacement() const { return sitePlacement_; }
    // The placement every element and annotation hangs off: the site's
    // frame, unmoved, shared.
    [[nodiscard]] Id elementPlacement() const { return elementPlacement_; }
    [[nodiscard]] Id axisContext() const { return axisContext_; }
    [[nodiscard]] Id bodyContext() const { return bodyContext_; }
    [[nodiscard]] Id footPrintContext() const { return footPrintContext_; }
    [[nodiscard]] Id annotationContext() const { return annotationContext_; }
    [[nodiscard]] Id metre() const { return metre_; }

    Id shape(Id context, std::string_view identifier, std::string_view type,
             const std::vector<Id>& items);
    Id productShape(const std::vector<Id>& representations);

    // An element or annotation instance of `ifcClass`, placed at
    // elementPlacement(), and contained in the site. `tag` is written where
    // the class has a Tag.
    Id product(const IfcClass& ifcClass, std::string_view key, std::string_view name,
               std::string_view description, Id representation, std::string_view tag = {});

    // ---- properties -------------------------------------------------------------
    // An IfcPropertySet, or 0 when the list is empty.
    Id propertySet(std::string_view key, std::string_view name, const PropertyList& properties);
    // IfcRelDefinesByProperties from `set` to `objects` (nothing when either
    // is empty). `key` names the relationship, as propertySet's key names
    // the set, so that its GlobalId survives a re-export.
    void defines(std::string_view key, Id set, const std::vector<Id>& objects);
    // The IfcPropertyEnumeration called `name`, written once.
    Id enumeration(std::string_view name, const std::vector<std::string>& encodedValues);

    // ---- relationships written by finish() ---------------------------------------
    void referenceInSite(Id product);
    void aggregateInProject(Id object);
    void layer(std::string_view name, Id representation);
    // An IfcClassification, written once per key.
    Id classification(std::string_view key, std::string_view source, std::string_view edition,
                      std::string_view name, std::string_view description);
    // A reference into it, written once per (classification, identification).
    Id classificationReference(Id classification, std::string_view identification,
                               std::string_view name);
    void associate(Id reference, Id object);
    // An IfcRelAssignsToGroup from `group` to `members`.
    void group(Id group, std::string_view key, const std::vector<Id>& members);

    // ---- presentation ----------------------------------------------------------------
    // Styles `item` with the colour: a curve style for a curve or text, a
    // surface style for a body. One style per colour and kind.
    void colourCurve(Id item, const entity::Color& colour);
    void colourSurface(Id item, const entity::Color& colour);

    void finish();

  private:
    void writeProjectFrame();

    const ExportOptions& options_;
    Frame frame_;
    IfcExport& report_;
    StepFile file_;
    std::set<std::string> guids_;

    Id origin2_ = 0;
    Id identity3_ = 0;
    Id unitLine_ = 0;
    Id project_ = 0;
    Id site_ = 0;
    Id sitePlacement_ = 0;
    Id elementPlacement_ = 0;
    Id axisContext_ = 0;
    Id bodyContext_ = 0;
    Id footPrintContext_ = 0;
    Id annotationContext_ = 0;
    Id metre_ = 0;

    std::vector<Id> contained_;
    std::vector<Id> referenced_;
    std::vector<Id> aggregatedInProject_;
    std::map<std::string, std::vector<Id>> layers_;
    std::map<std::string, Id> classifications_;
    std::map<std::pair<Id, std::string>, Id> references_;
    std::map<Id, std::string> referenceKeys_; // a reference -> "<classification>/<identification>"
    std::map<Id, std::vector<Id>> associations_;
    std::map<Id, std::string> classificationKeys_;
    // source, entity, predefined type, object type, system, why -> count
    std::map<std::array<std::string, 6>, std::size_t> tally_;
    std::map<std::string, Id> enumerations_;
    std::map<std::string, Id> curveStyles_;
    std::map<std::string, Id> surfaceStyles_;
};

} // namespace katana::ifc::detail
