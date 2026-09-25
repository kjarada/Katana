#include "builder.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>

#include "katana/core/text.hpp"
#include "katana/ifc/step.hpp"
#include "product_layout.hpp"

namespace katana::ifc::detail {

namespace {

// IfcLabel and IfcIdentifier are STRING(255).
constexpr std::size_t kLabelLimit = 255;

// The precision of the model context: the distance below which two points
// are one, which the continuity checks of the buildingSMART validation
// service also take as their tolerance (ALS016, ALS017). A tenth of a
// millimetre is a hundred times finer than any survey Katana reduces and
// far coarser than the arithmetic's error at the local coordinates written.
constexpr double kContextPrecision = 1e-5;

std::string labelOrText(std::string_view value)
{
    return characterCount(value) > kLabelLimit ? typedValue("IfcText", stepString(value))
                                               : typedValue("IfcLabel", stepString(value));
}

std::string colourKey(const entity::Color& colour)
{
    return colour.toHex();
}

} // namespace

// ---- PropertyList -----------------------------------------------------------------

PropertyList& PropertyList::add(Property property)
{
    items_.push_back(std::move(property));
    return *this;
}

PropertyList& PropertyList::label(std::string_view name, std::string_view value,
                                  std::string_view specification)
{
    if (value.empty()) {
        return *this;
    }
    return add({std::string(name), labelOrText(value), std::string(specification)});
}

PropertyList& PropertyList::text(std::string_view name, std::string_view value)
{
    if (value.empty()) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcText", stepString(value)), {}});
}

PropertyList& PropertyList::identifier(std::string_view name, std::string_view value)
{
    if (value.empty()) {
        return *this;
    }
    if (characterCount(value) > kLabelLimit) {
        return text(name, value);
    }
    return add({std::string(name), typedValue("IfcIdentifier", stepString(value)), {}});
}

PropertyList& PropertyList::length(std::string_view name, std::optional<double> value)
{
    if (!value || !std::isfinite(*value)) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcLengthMeasure", stepReal(*value)), {}});
}

PropertyList& PropertyList::positiveLength(std::string_view name, std::optional<double> value)
{
    if (!value || !std::isfinite(*value) || *value <= 0.0) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcPositiveLengthMeasure", stepReal(*value)), {}});
}

PropertyList& PropertyList::ratio(std::string_view name, std::optional<double> value)
{
    if (!value || !std::isfinite(*value)) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcRatioMeasure", stepReal(*value)), {}});
}

PropertyList& PropertyList::positiveRatio(std::string_view name, std::optional<double> value)
{
    if (!value || !std::isfinite(*value) || *value <= 0.0) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcPositiveRatioMeasure", stepReal(*value)), {}});
}

PropertyList& PropertyList::real(std::string_view name, std::optional<double> value)
{
    if (!value || !std::isfinite(*value)) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcReal", stepReal(*value)), {}});
}

PropertyList& PropertyList::integer(std::string_view name, std::optional<long long> value)
{
    if (!value) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcInteger", std::to_string(*value)), {}});
}

PropertyList& PropertyList::boolean(std::string_view name, std::optional<bool> value)
{
    if (!value) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcBoolean", *value ? ".T." : ".F."), {}});
}

PropertyList& PropertyList::date(std::string_view name, std::string_view isoDate)
{
    if (isoDate.empty()) {
        return *this;
    }
    return add({std::string(name), typedValue("IfcDate", stepString(isoDate)), {}});
}

PropertyList& PropertyList::enumerated(std::string_view name, std::string_view label,
                                       Id enumeration, std::string_view specification)
{
    if (label.empty()) {
        return *this;
    }
    Property property{std::string(name), typedValue("IfcLabel", stepString(label)),
                      std::string(specification)};
    property.enumerated = true;
    property.enumeration = enumeration;
    return add(std::move(property));
}

void addEntityProperties(PropertyList& list, const entity::PropertyMap& properties)
{
    for (const auto& [name, value] : properties) {
        if (name.empty() || characterCount(name) > kLabelLimit) {
            continue;
        }
        if (const auto* text = std::get_if<std::string>(&value)) {
            list.label(name, *text);
        } else if (const auto* real = std::get_if<double>(&value)) {
            list.real(name, *real);
        } else if (const auto* whole = std::get_if<std::int64_t>(&value)) {
            list.integer(name, *whole);
        } else if (const auto* flag = std::get_if<bool>(&value)) {
            list.boolean(name, *flag);
        }
    }
}

// ---- Builder ------------------------------------------------------------------------

Builder::Builder(const ExportOptions& options, Frame frame, IfcExport& report)
    : options_(options), frame_(frame), report_(report)
{
    writeProjectFrame();
}

std::string Builder::guid(std::string_view key)
{
    const std::string space =
        options_.guidNamespace.empty() ? options_.projectName : options_.guidNamespace;
    std::string candidate = guidFor(space, key);
    for (int attempt = 1; !guids_.insert(candidate).second; ++attempt) {
        candidate = guidFor(space, std::string(key) + "#" + std::to_string(attempt));
    }
    return candidate;
}

Vec2 Builder::local(Vec2 world) const
{
    return Vec2(world.x - frame_.origin.x, world.y - frame_.origin.y);
}

Vec3 Builder::local(Vec3 world) const
{
    return Vec3(world.x - frame_.origin.x, world.y - frame_.origin.y, world.z - frame_.origin.z);
}

Id Builder::point(Vec2 world)
{
    return localPoint(local(world));
}

Id Builder::localPoint(Vec2 at)
{
    return file_.add("IfcCartesianPoint", Args().reals({at.x, at.y}));
}

Id Builder::point(Vec3 world)
{
    const Vec3 at = local(world);
    return file_.add("IfcCartesianPoint", Args().reals({at.x, at.y, at.z}));
}

Id Builder::direction(double x, double y)
{
    return file_.add("IfcDirection", Args().reals({x, y}));
}

Id Builder::direction(double x, double y, double z)
{
    return file_.add("IfcDirection", Args().reals({x, y, z}));
}

Id Builder::origin2()
{
    if (origin2_ == 0) {
        origin2_ = file_.add("IfcCartesianPoint", Args().reals({0.0, 0.0}));
    }
    return origin2_;
}

Id Builder::placement2(Id location, double angle)
{
    return file_.add("IfcAxis2Placement2D",
                     Args().ref(location).ref(direction(std::cos(angle), std::sin(angle))));
}

Id Builder::identity3()
{
    if (identity3_ == 0) {
        const Id at = file_.add("IfcCartesianPoint", Args().reals({0.0, 0.0, 0.0}));
        identity3_ = file_.add("IfcAxis2Placement3D", Args().ref(at).null().null());
    }
    return identity3_;
}

Id Builder::unitLine()
{
    if (unitLine_ == 0) {
        const Id vector = file_.add("IfcVector", Args().ref(direction(1.0, 0.0)).real(1.0));
        unitLine_ = file_.add("IfcLine", Args().ref(origin2()).ref(vector));
    }
    return unitLine_;
}

Id Builder::polyline(const std::vector<Vec2>& world, bool closed)
{
    std::vector<Id> points;
    points.reserve(world.size() + 1);
    for (const Vec2& p : world) {
        points.push_back(point(p));
    }
    if (closed && !points.empty()) {
        points.push_back(points.front()); // IfcPolyline closes by repeating its first point
    }
    return file_.add("IfcPolyline", Args().refs(points));
}

Id Builder::polyline(const std::vector<Vec3>& world, bool closed)
{
    std::vector<Id> points;
    points.reserve(world.size() + 1);
    for (const Vec3& p : world) {
        points.push_back(point(p));
    }
    if (closed && !points.empty()) {
        points.push_back(points.front());
    }
    return file_.add("IfcPolyline", Args().refs(points));
}

void Builder::writeProjectFrame()
{
    // Units: SI, lengths in metres and angles in radians - what every length
    // and angle in Katana already is, so nothing is scaled on the way out.
    // IfcSIUnit's Dimensions is derived from the unit type.
    const auto unit = [&](std::string_view type, std::string_view name) {
        return file_.add("IfcSIUnit", Args().derived().enumeration(type).null().enumeration(name));
    };
    metre_ = unit("LENGTHUNIT", "METRE");
    const Id squareMetre = unit("AREAUNIT", "SQUARE_METRE");
    const Id cubicMetre = unit("VOLUMEUNIT", "CUBIC_METRE");
    const Id radian = unit("PLANEANGLEUNIT", "RADIAN");
    const Id units =
        file_.add("IfcUnitAssignment", Args().refs({metre_, squareMetre, cubicMetre, radian}));

    // One 3D model context; true north is left to its default, grid north
    // (+y), which is what a projected coordinate system's y axis is.
    const Id context = file_.add(
        "IfcGeometricRepresentationContext",
        Args().null().string("Model").integer(3).real(kContextPrecision).ref(identity3()).null());
    // The subcontexts the representations name. A subcontext's dimension,
    // precision, coordinate system and true north are its parent's
    // (derived, *). Axis is a GRAPH_VIEW, as the IFC 4.3 alignment
    // examples have it.
    const auto subContext = [&](std::string_view identifier, std::string_view view) {
        return file_.add("IfcGeometricRepresentationSubContext", Args()
                                                                     .string(identifier)
                                                                     .string("Model")
                                                                     .derived()
                                                                     .derived()
                                                                     .derived()
                                                                     .derived()
                                                                     .ref(context)
                                                                     .null()
                                                                     .enumeration(view)
                                                                     .null());
    };
    axisContext_ = subContext("Axis", "GRAPH_VIEW");
    bodyContext_ = subContext("Body", "MODEL_VIEW");
    footPrintContext_ = subContext("FootPrint", "MODEL_VIEW");
    annotationContext_ = subContext("Annotation", "MODEL_VIEW");

    if (frame_.georeferenced) {
        const Georeference& crs = options_.georeference;
        const Id projected = file_.add("IfcProjectedCRS", Args()
                                                              .string(crs.name)
                                                              .stringOrNull(crs.description)
                                                              .stringOrNull(crs.geodeticDatum)
                                                              .stringOrNull(crs.verticalDatum)
                                                              .stringOrNull(crs.mapProjection)
                                                              .stringOrNull(crs.mapZone)
                                                              .ref(metre_));
        // The file's axes are the grid's (no rotation: abscissa 1, ordinate
        // 0) at the grid's scale: Katana's coordinates ARE grid coordinates,
        // so no ground-to-grid factor applies, and the length units are the
        // CRS's (metres), so the scale is 1 (GRF005).
        file_.add("IfcMapConversion", Args()
                                          .ref(context)
                                          .ref(projected)
                                          .real(frame_.origin.x)
                                          .real(frame_.origin.y)
                                          .real(frame_.origin.z)
                                          .real(1.0)
                                          .real(0.0)
                                          .real(1.0));
    }

    project_ = file_.add("IfcProject", Args()
                                           .string(guid("project"))
                                           .null()
                                           .string(options_.projectName)
                                           .stringOrNull(options_.projectDescription)
                                           .null()
                                           .null()
                                           .null()
                                           .refs({context})
                                           .ref(units));
    sitePlacement_ = file_.add("IfcLocalPlacement", Args().null().ref(identity3()));
    site_ = file_.add("IfcSite", Args()
                                     .string(guid("site"))
                                     .null()
                                     .string(options_.siteName)
                                     .null()
                                     .null()
                                     .ref(sitePlacement_)
                                     .null()
                                     .null()
                                     .enumeration("ELEMENT")
                                     .null()
                                     .null()
                                     .null()
                                     .null()
                                     .null());
    elementPlacement_ = file_.add("IfcLocalPlacement", Args().ref(sitePlacement_).ref(identity3()));
}

Id Builder::shape(Id context, std::string_view identifier, std::string_view type,
                  const std::vector<Id>& items)
{
    return file_.add("IfcShapeRepresentation",
                     Args().ref(context).string(identifier).string(type).refs(items));
}

Id Builder::productShape(const std::vector<Id>& representations)
{
    return file_.add("IfcProductDefinitionShape", Args().null().null().refs(representations));
}

Id Builder::product(const IfcClass& ifcClass, std::string_view key, std::string_view name,
                    std::string_view description, Id representation, std::string_view tag)
{
    const ProductLayout layout = productLayout(ifcClass.entity);
    Args args;
    args.string(guid(key))
        .null()
        .stringOrNull(name)
        .stringOrNull(description)
        .stringOrNull(ifcClass.objectType)
        .ref(elementPlacement_)
        .refOrNull(representation);
    if (layout.hasTag) {
        args.stringOrNull(tag);
    }
    if (layout.hasPredefinedType) {
        args.enumerationOrNull(ifcClass.predefinedType);
    }
    const Id id = file_.add(ifcClass.entity, args);
    contained_.push_back(id);
    ++report_.classes[ifcClass.entity];
    return id;
}

Id Builder::propertySet(std::string_view key, std::string_view name, const PropertyList& properties)
{
    if (properties.empty()) {
        return 0;
    }
    std::vector<Id> ids;
    ids.reserve(properties.items().size());
    for (const Property& property : properties.items()) {
        Args args;
        args.string(property.name).stringOrNull(property.specification);
        if (property.enumerated) {
            args.raw(listOf({property.value})).refOrNull(property.enumeration);
            ids.push_back(file_.add("IfcPropertyEnumeratedValue", args));
        } else {
            if (property.value.empty()) {
                args.null();
            } else {
                args.raw(property.value);
            }
            args.null();
            ids.push_back(file_.add("IfcPropertySingleValue", args));
        }
    }
    return file_.add("IfcPropertySet", Args()
                                           .string(guid(std::string("pset/") + std::string(key)))
                                           .null()
                                           .string(name)
                                           .null()
                                           .refs(ids));
}

void Builder::defines(std::string_view key, Id set, const std::vector<Id>& objects)
{
    if (set == 0 || objects.empty()) {
        return;
    }
    file_.add("IfcRelDefinesByProperties", Args()
                                               .string(guid("defines/" + std::string(key)))
                                               .null()
                                               .null()
                                               .null()
                                               .refs(objects)
                                               .ref(set));
}

Id Builder::enumeration(std::string_view name, const std::vector<std::string>& encodedValues)
{
    const auto found = enumerations_.find(std::string(name));
    if (found != enumerations_.end()) {
        return found->second;
    }
    const Id id =
        file_.add("IfcPropertyEnumeration", Args().string(name).raw(listOf(encodedValues)).null());
    enumerations_.emplace(std::string(name), id);
    return id;
}

void Builder::referenceInSite(Id product)
{
    referenced_.push_back(product);
}

void Builder::aggregateInProject(Id object)
{
    aggregatedInProject_.push_back(object);
}

void Builder::layer(std::string_view name, Id representation)
{
    if (!name.empty() && representation != 0) {
        layers_[std::string(name)].push_back(representation);
    }
}

Id Builder::classification(std::string_view key, std::string_view source, std::string_view edition,
                           std::string_view name, std::string_view description)
{
    const auto found = classifications_.find(std::string(key));
    if (found != classifications_.end()) {
        return found->second;
    }
    const Id id = file_.add("IfcClassification", Args()
                                                     .stringOrNull(source)
                                                     .stringOrNull(edition)
                                                     .null()
                                                     .string(name)
                                                     .stringOrNull(description)
                                                     .null()
                                                     .null());
    classifications_.emplace(std::string(key), id);
    classificationKeys_.emplace(id, std::string(key));
    return id;
}

Id Builder::classificationReference(Id classification, std::string_view identification,
                                    std::string_view name)
{
    const auto key = std::make_pair(classification, std::string(identification));
    const auto found = references_.find(key);
    if (found != references_.end()) {
        return found->second;
    }
    const Id id = file_.add(
        "IfcClassificationReference",
        Args().null().string(identification).stringOrNull(name).ref(classification).null().null());
    references_.emplace(key, id);
    referenceKeys_.emplace(id,
                           classificationKeys_[classification] + "/" + std::string(identification));
    return id;
}

void Builder::associate(Id reference, Id object)
{
    if (reference != 0 && object != 0) {
        associations_[reference].push_back(object);
    }
}

void Builder::group(Id group, std::string_view key, const std::vector<Id>& members)
{
    if (members.empty()) {
        return;
    }
    file_.add("IfcRelAssignsToGroup", Args()
                                          .string(guid(std::string("group/") + std::string(key)))
                                          .null()
                                          .null()
                                          .null()
                                          .refs(members)
                                          .null()
                                          .ref(group));
}

void Builder::colourCurve(Id item, const entity::Color& colour)
{
    const std::string key = colourKey(colour);
    auto found = curveStyles_.find(key);
    if (found == curveStyles_.end()) {
        const Id rgb = file_.add(
            "IfcColourRgb",
            Args().null().real(colour.r / 255.0).real(colour.g / 255.0).real(colour.b / 255.0));
        const Id style =
            file_.add("IfcCurveStyle", Args().string(key).null().null().ref(rgb).boolean(true));
        found = curveStyles_.emplace(key, style).first;
    }
    file_.add("IfcStyledItem", Args().ref(item).refs({found->second}).null());
}

void Builder::colourSurface(Id item, const entity::Color& colour)
{
    const std::string key = colourKey(colour);
    auto found = surfaceStyles_.find(key);
    if (found == surfaceStyles_.end()) {
        const Id rgb = file_.add(
            "IfcColourRgb",
            Args().null().real(colour.r / 255.0).real(colour.g / 255.0).real(colour.b / 255.0));
        const Id shading = file_.add("IfcSurfaceStyleShading", Args().ref(rgb).null());
        const Id style =
            file_.add("IfcSurfaceStyle", Args().string(key).enumeration("BOTH").refs({shading}));
        found = surfaceStyles_.emplace(key, style).first;
    }
    file_.add("IfcStyledItem", Args().ref(item).refs({found->second}).null());
}

void Builder::finish()
{
    // The site, and every alignment: IFC 4.3 aggregates an alignment into
    // the project and does not contain it in a spatial element (ALB004,
    // SPS007).
    std::vector<Id> parts{site_};
    parts.insert(parts.end(), aggregatedInProject_.begin(), aggregatedInProject_.end());
    file_.add(
        "IfcRelAggregates",
        Args().string(guid("aggregates/project")).null().null().null().ref(project_).refs(parts));
    if (!contained_.empty()) {
        file_.add(
            "IfcRelContainedInSpatialStructure",
            Args().string(guid("contained/site")).null().null().null().refs(contained_).ref(site_));
    }
    if (!referenced_.empty()) {
        file_.add("IfcRelReferencedInSpatialStructure", Args()
                                                            .string(guid("referenced/site"))
                                                            .null()
                                                            .null()
                                                            .null()
                                                            .refs(referenced_)
                                                            .ref(site_));
    }
    for (const auto& [name, representations] : layers_) {
        file_.add("IfcPresentationLayerAssignment",
                  Args().string(name).null().refs(representations).null());
    }
    for (const auto& [reference, objects] : associations_) {
        file_.add("IfcRelAssociatesClassification",
                  Args()
                      .string(guid("classified/" + referenceKeys_[reference]))
                      .null()
                      .null()
                      .null()
                      .refs(objects)
                      .ref(reference));
    }
}

} // namespace katana::ifc::detail
