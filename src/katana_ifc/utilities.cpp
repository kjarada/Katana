// Services of an AS 5488 subsurface utility investigation -> IFC.
//
// A SERVICE is an IfcDistributionSystem (WATERSUPPLY, SEWAGE, ELECTRICAL ...,
// classification.hpp) grouping:
//
//   * one element per GRADED SEGMENT - an IfcPipeSegment, IfcCableSegment,
//     IfcCableCarrierSegment or IfcDistributionFlowElement - because AS 5488 grades a
//     segment, not a service: a water main verified by a pothole is QL-A for
//     a metre and QL-B either side, and a single element carrying one level
//     would claim too much or too little along most of its length
//     (docs/subsurface_utilities.md, "Results are per segment");
//   * one IfcAnnotation SURVEY per LOCATED POINT, where the evidence is: its
//     method, the level it supports, what it claims, its assessed
//     uncertainty and the depth of cover there;
//   * for a service recorded at one point, the element its feature names -
//     a pit, a valve, a hydrant (classifyUtilityPoint).
//
// What each carries:
//
//   AS5488_QualityLevel   the segment's graded level, the level claimed for
//                         it, whether the claim holds, what limits it, the
//                         evidence along it, its plan length
//   AS5488_LocatedPoint   the point's method, attained and claimed levels,
//                         the reasons it is below its method's ceiling,
//                         levels, depth and cover
//   AS5488_Service        the service: type, owner, material, size,
//                         configuration, status, and its length at each level
//   Pset_Uncertainty      IFC's own statement of positional uncertainty: for
//                         a segment the AS 5488 tolerance of its level, for a
//                         point its assessed uncertainty; MEASUREMENT for
//                         QL-A and QL-B, INTERPRETATION for QL-C (a surface
//                         feature correlated with records), ESTIMATE for QL-D
//   Pset_<class>TypeCommon  Reference (the asset identifier), Status
//                         (EXISTING, NEW ...), the diameter
//   Pset_ConstructionOccurence  AssetIdentifier, InstallationDate
//   TfNSW_UtilitySchema   the delivery schema's attributes exactly as the
//                         schedule wrote them (UtilityAttributes::written) -
//                         by the schema's order and labels, and as
//                         enumerated values of its domains, when the schema
//                         is given
//
// and each is classified: IfcClassificationReference "QL-A" .. "QL-D" in
// AS 5488.1-2019, and the asset type code in the TfNSW Utility Schema.
//
// GEOMETRY IS NEVER GUESSED. A segment has an Axis in 3D - and a Body, a
// swept disk of its size - only when both of its ends give the level of the
// service's CENTRE: a level recorded on the centre, or on the top or invert
// with a size to bring it to the centre. Otherwise its FootPrint is written
// in plan and nothing else: a pipe drawn at a depth nobody measured would be
// taken for one somebody had.

#include <algorithm>
#include <cmath>
#include <cstdio>

#include "katana/core/text.hpp"
#include "katana/survey/subsurface/quality_level.hpp"
#include "parts.hpp"

namespace katana::ifc::detail {

namespace sub = katana::survey::subsurface;

namespace {

// The attributes of the TfNSW Utility Schema that the schedule reader
// interprets (utility_csv.hpp accepts them as aliases of its own columns):
// per service, and per point. The rest arrive already by their schema names
// in UtilityAttributes::fields and UtilityVertex::fields.
constexpr std::array<std::string_view, 7> kSchemaServiceColumns{
    "AssetIdentifier", "AssetTypeCode", "AssetOwner", "AssetStatus", "Size",
    "Material",        "Configuration"};
constexpr std::array<std::string_view, 4> kSchemaPointColumns{"LocateMethod", "DepthLocation",
                                                              "Depth", "QualityLevel"};

constexpr std::string_view kStandard = "AS 5488.1-2019";

std::string qualityName(sub::QualityLevel level)
{
    switch (level) {
    case sub::QualityLevel::A:
        return "Quality Level A";
    case sub::QualityLevel::B:
        return "Quality Level B";
    case sub::QualityLevel::C:
        return "Quality Level C";
    case sub::QualityLevel::D:
        return "Quality Level D";
    }
    return {};
}

// IFC's PEnum_UncertaintyBasis for what a quality level rests on.
std::string_view uncertaintyBasis(sub::QualityLevel level)
{
    switch (level) {
    case sub::QualityLevel::A:
    case sub::QualityLevel::B:
        return "MEASUREMENT";
    case sub::QualityLevel::C:
        return "INTERPRETATION";
    case sub::QualityLevel::D:
        return "ESTIMATE";
    }
    return "NOTKNOWN";
}

// PEnum_ElementStatus. A disused or abandoned service is still in the
// ground, which is what EXISTING says; what it is used for is AS5488_Service's
// Status.
std::string_view elementStatus(sub::UtilityStatus status)
{
    switch (status) {
    case sub::UtilityStatus::InService:
    case sub::UtilityStatus::Disused:
    case sub::UtilityStatus::Abandoned:
        return "EXISTING";
    case sub::UtilityStatus::Proposed:
        return "NEW";
    case sub::UtilityStatus::Unknown:
        break;
    }
    return "NOTKNOWN";
}

// "1998/06/01", as the TfNSW schema writes a date, as an IfcDate
// ("1998-06-01"); empty for anything else.
std::string isoDate(std::string_view text)
{
    text = core::trimmed(text);
    if (text.size() != 10 || (text[4] != '/' && text[4] != '-') || text[7] != text[4]) {
        return {};
    }
    for (const std::size_t i : {0u, 1u, 2u, 3u, 5u, 6u, 8u, 9u}) {
        if (text[i] < '0' || text[i] > '9') {
            return {};
        }
    }
    std::string out(text);
    out[4] = out[7] = '-';
    return out;
}

// The level of the service's centre at `vertex`, when it can be had without
// assuming anything: see the file's header.
std::optional<double> centreLevel(const sub::UtilityVertex& vertex, double diameter)
{
    const auto level = sub::serviceLevel(vertex);
    if (!level) {
        return std::nullopt;
    }
    switch (vertex.levelReference) {
    case sub::LevelReference::Centre:
        return level;
    case sub::LevelReference::Top:
        return diameter > 0.0 ? std::optional(*level - diameter / 2.0) : std::nullopt;
    case sub::LevelReference::Invert:
        return diameter > 0.0 ? std::optional(*level + diameter / 2.0) : std::nullopt;
    case sub::LevelReference::Unknown:
        break;
    }
    return std::nullopt;
}

std::string levelReferenceName(sub::LevelReference reference)
{
    return sub::toString(reference);
}

Vec2 planOf(const sub::UtilityVertex& vertex)
{
    return Vec2(vertex.position.easting, vertex.position.northing);
}

std::string joined(const std::vector<std::string>& parts, std::string_view separator)
{
    std::string out;
    for (const std::string& part : parts) {
        if (!out.empty()) {
            out += separator;
        }
        out += part;
    }
    return out;
}

// The common property set IFC 4.3 defines for a segment class, if any.
std::string_view commonPropertySet(std::string_view entity)
{
    if (entity == "IfcPipeSegment") {
        return "Pset_PipeSegmentTypeCommon";
    }
    if (entity == "IfcCableSegment") {
        return "Pset_CableSegmentTypeCommon";
    }
    if (entity == "IfcCableCarrierSegment") {
        return "Pset_CableCarrierSegmentTypeCommon";
    }
    if (entity == "IfcDistributionChamberElement") {
        return "Pset_DistributionChamberElementCommon";
    }
    return {};
}

// The delivery schema's property set name: TfNSW's, or one made from another
// client's schema title ("Acme Utility Spec" -> "Acme_Utility_Spec").
std::string deliveryPropertySetName(const sub::DeliverySchema* schema)
{
    if (!schema || schema->title.empty() || schema->title.starts_with("TfNSW")) {
        return "TfNSW_UtilitySchema";
    }
    std::string name;
    bool gap = false;
    for (const char c : schema->title) {
        if (std::isalnum(static_cast<unsigned char>(c)) != 0) {
            if (gap && !name.empty()) {
                name.push_back('_');
            }
            gap = false;
            name.push_back(c);
        } else {
            gap = true;
        }
    }
    // PSE002: a property set of one's own must not start with "Pset".
    if (name.empty() || core::lowered(name).starts_with("pset")) {
        name = "Delivery_" + name;
    }
    return name;
}

class UtilityWriter {
  public:
    UtilityWriter(Builder& builder, const UtilityInput& input) : b_(builder), input_(input) {}

    void write()
    {
        if (input_.lines.empty()) {
            return;
        }
        standard_ =
            b_.classification("as5488", "Standards Australia", "2019", std::string(kStandard),
                              "Classification of subsurface utility information: quality "
                              "levels A to D");
        for (const sub::UtilityLine& line : input_.lines) {
            writeService(line);
        }
    }

  private:
    // True when the schedule is written to the delivery schema: it names
    // its services by AssetIdentifier, or the caller gave the schema.
    [[nodiscard]] bool deliverySchedule(const sub::UtilityLine& line) const
    {
        return input_.schema != nullptr || line.attributes.written.contains("AssetIdentifier");
    }

    Id schemaClassification()
    {
        if (schemaClassification_ == 0) {
            const sub::DeliverySchema* schema = input_.schema;
            const bool tfnsw = schema == nullptr || schema->title.starts_with("TfNSW");
            schemaClassification_ = b_.classification(
                "delivery", tfnsw ? "Transport for NSW" : "",
                schema ? schema->version : std::string(),
                schema && !schema->title.empty() ? schema->title
                                                 : "TfNSW Utility Schema and Specification",
                "Asset type codes (AS 5488.2 Table A.4) of the delivery schema");
        }
        return schemaClassification_;
    }

    Id qualityReference(sub::QualityLevel level)
    {
        return b_.classificationReference(standard_, sub::toString(level), qualityName(level));
    }

    void writeService(const sub::UtilityLine& line)
    {
        const std::string key = "utility/" + line.id;
        const sub::UtilityAttributes& service = line.attributes;
        if (line.vertices.empty()) {
            b_.warn("service " + line.id + " has no points and is not written");
            return;
        }

        std::optional<sub::GradedLine> graded;
        std::vector<sub::CoverResult> covers;
        if (line.vertices.size() >= 2) {
            auto grading = sub::gradeLine(line, input_.grading);
            if (!grading) {
                b_.warn("service " + line.id + " is not written: " + grading.error().describe());
                return;
            }
            graded = std::move(*grading);
            if (auto cover = sub::depthOfCover(line, std::nullopt, input_.grading)) {
                covers = std::move(*cover);
            }
        }

        const UtilityClass run = classifyUtilityRun(service);
        const std::string systemName =
            run.system == "USERDEFINED" ? run.systemObjectType : run.system;
        const Id system =
            b_.file().add("IfcDistributionSystem", Args()
                                                       .string(b_.guid(key))
                                                       .null()
                                                       .string(line.id)
                                                       .stringOrNull(service.description)
                                                       .stringOrNull(run.systemObjectType)
                                                       .stringOrNull(longName(service))
                                                       .enumeration(run.system));
        ++b_.report().classes["IfcDistributionSystem"];
        b_.referenceInSite(system);
        ++b_.report().services;

        std::vector<Id> members;
        std::vector<Id> elements; // the segments or point feature: what the service's sets describe
        if (graded) {
            for (std::size_t s = 0; s < graded->segments.size(); ++s) {
                elements.push_back(writeSegment(line, *graded, graded->segments[s], run, key));
            }
        } else {
            const std::optional<UtilityClass> feature = classifyUtilityPoint(service);
            if (feature) {
                elements.push_back(writeFeature(line, *feature, key));
            } else {
                b_.warn("service " + line.id +
                        " is one point and its feature names no element IFC has; it is written "
                        "as its located point only");
            }
        }
        members.insert(members.end(), elements.begin(), elements.end());
        for (std::size_t v = 0; v < line.vertices.size(); ++v) {
            members.push_back(writeLocatedPoint(line, graded, covers, v, key));
        }
        b_.group(system, key, members);

        // What describes the service as a whole, on the system and on each
        // of its elements, so that whichever a person picks says it.
        std::vector<Id> described{system};
        described.insert(described.end(), elements.begin(), elements.end());
        b_.defines(
            key + "/service",
            b_.propertySet(key + "/service", "AS5488_Service", serviceProperties(service, graded)),
            described);
        PropertyList reference;
        reference.identifier("Reference", line.id);
        b_.defines(key + "/system",
                   b_.propertySet(key + "/system", "Pset_DistributionSystemCommon", reference),
                   {system});
        PropertyList construction;
        construction.label("AssetIdentifier", line.id);
        if (const auto installed = service.fields.find("UtilityInstallDate");
            installed != service.fields.end()) {
            construction.date("InstallationDate", isoDate(installed->second));
        }
        b_.defines(
            key + "/construction",
            b_.propertySet(key + "/construction", "Pset_ConstructionOccurence", construction),
            elements);
        if (deliverySchedule(line)) {
            b_.defines(key + "/delivery",
                       b_.propertySet(key + "/delivery", deliveryPropertySetName(input_.schema),
                                      deliveryProperties(service.written, service.fields,
                                                         kSchemaServiceColumns)),
                       described);
            const std::string code = [&] {
                const auto written = service.written.find("AssetTypeCode");
                return written != service.written.end() ? written->second
                                                        : std::string(assetTypeCode(service.type));
            }();
            if (!code.empty()) {
                const Id typeReference = b_.classificationReference(schemaClassification(), code,
                                                                    sub::toString(service.type));
                for (const Id object : described) {
                    b_.associate(typeReference, object);
                }
            }
        }
    }

    [[nodiscard]] static std::string longName(const sub::UtilityAttributes& service)
    {
        std::vector<std::string> parts;
        if (!service.owner.empty()) {
            parts.push_back(service.owner);
        }
        parts.emplace_back(sub::toString(service.type));
        return joined(parts, " ");
    }

    Id writeSegment(const sub::UtilityLine& line, const sub::GradedLine& graded,
                    const sub::GradedSegment& segment, const UtilityClass& run,
                    const std::string& key)
    {
        const sub::UtilityAttributes& service = line.attributes;
        const sub::UtilityVertex& from = line.vertices[segment.from];
        const sub::UtilityVertex& to = line.vertices[segment.from + 1];
        const double size = service.diameter;
        const auto startCentre = centreLevel(from, size);
        const auto endCentre = centreLevel(to, size);
        const bool in3d = startCentre && endCentre;

        std::vector<Id> representations;
        const std::string layer = std::string("Utilities/") + sub::toString(service.type) + "/" +
                                  sub::toString(segment.level);
        if (in3d) {
            const Id axis = b_.polyline(
                std::vector<Vec3>{Vec3(from.position.easting, from.position.northing, *startCentre),
                                  Vec3(to.position.easting, to.position.northing, *endCentre)},
                false);
            representations.push_back(b_.shape(b_.axisContext(), "Axis", "Curve3D", {axis}));
            if (size > 0.0) {
                const Id body = b_.file().add(
                    "IfcSweptDiskSolid", Args().ref(axis).real(size / 2.0).null().null().null());
                representations.push_back(
                    b_.shape(b_.bodyContext(), "Body", "AdvancedSweptSolid", {body}));
            }
            ++b_.report().segmentsIn3d;
        } else {
            const Id footprint = b_.polyline(std::vector<Vec2>{planOf(from), planOf(to)}, false);
            representations.push_back(
                b_.shape(b_.footPrintContext(), "FootPrint", "Curve2D", {footprint}));
        }
        for (const Id representation : representations) {
            b_.layer(layer, representation);
        }
        const std::string segmentKey = key + "/segment/" + std::to_string(segment.from);
        const std::string name = line.id + " " + from.id + " to " + to.id;
        const Id element = b_.product(run.element, segmentKey, name,
                                      std::string(kStandard) + " " + sub::toString(segment.level) +
                                          " segment of " + line.id,
                                      b_.productShape(representations), line.id);
        ++b_.report().serviceSegments;
        b_.tally("service " + line.id, run.element,
                 run.system == "USERDEFINED" ? run.systemObjectType : run.system,
                 "a graded segment: " + run.reason);

        // ---- AS 5488: the grade -----------------------------------------------------
        PropertyList grade;
        grade.label("QualityLevel", sub::toString(segment.level),
                    "The AS 5488.1-2019 quality level the segment's evidence supports");
        const auto& start = graded.vertices[segment.from];
        const auto& end = graded.vertices[segment.from + 1];
        if (from.claimed && to.claimed) {
            const sub::QualityLevel claimed = std::min(*from.claimed, *to.claimed);
            grade.label("QualityLevelClaimed", sub::toString(claimed),
                        "The weaker of the levels the deliverable claims for its two ends");
            grade.boolean("ClaimSupported", claimed <= segment.level);
        }
        grade.text("LimitedBy", segment.limitedBy);
        const sub::PathEvidence evidence = line.pathEvidence.empty()
                                               ? sub::PathEvidence::Detected
                                               : line.pathEvidence[segment.from];
        grade.label("PathEvidence", sub::toString(evidence),
                    "What was observed between the two points");
        grade.identifier("StartPoint", from.id).identifier("EndPoint", to.id);
        grade.length("PlanLength", segment.length);
        grade.label("StartLevelReference",
                    sub::serviceLevel(from) ? levelReferenceName(from.levelReference) : "");
        grade.label("EndLevelReference",
                    sub::serviceLevel(to) ? levelReferenceName(to.levelReference) : "");
        grade.length("StartCentreLevel", startCentre).length("EndCentreLevel", endCentre);
        if (sub::hasVerticalMeasurement(from) && sub::hasVerticalMeasurement(to)) {
            grade.boolean("LevelsQualified",
                          start.classification.levelQualified && end.classification.levelQualified);
        }
        grade.boolean("DrawnIn3D", in3d);
        grade.label("Standard", kStandard);
        b_.defines(segmentKey + "/grade",
                   b_.propertySet(segmentKey + "/grade", "AS5488_QualityLevel", grade), {element});

        // ---- IFC's uncertainty: the tolerance the level certifies ---------------
        const sub::Tolerance& tolerance = input_.grading.tolerances.of(segment.level);
        PropertyList uncertainty;
        uncertainty.enumerated("UncertaintyBasis", uncertaintyBasis(segment.level));
        uncertainty.text("UncertaintyDescription",
                         std::string(kStandard) + " " + sub::toString(segment.level) +
                             (tolerance.horizontal ? ": the tolerance of the quality level"
                                                   : ": the quality level states no tolerance"));
        uncertainty.positiveLength("HorizontalUncertainty", tolerance.horizontal);
        if (in3d && start.classification.levelQualified && end.classification.levelQualified) {
            uncertainty.positiveLength("VerticalUncertainty", tolerance.vertical);
        }
        b_.defines(segmentKey + "/uncertainty",
                   b_.propertySet(segmentKey + "/uncertainty", "Pset_Uncertainty", uncertainty),
                   {element});

        // ---- the class's own common set -------------------------------------------
        if (const std::string_view common = commonPropertySet(run.element.entity);
            !common.empty()) {
            PropertyList list;
            list.identifier("Reference", line.id);
            list.enumerated("Status", elementStatus(service.status));
            if (run.element.entity == "IfcPipeSegment" && size > 0.0) {
                list.positiveLength(service.diameterIsInside ? "InnerDiameter" : "OuterDiameter",
                                    size);
                if (in3d) {
                    const double rise = *endCentre - *startCentre;
                    list.positiveLength("Length",
                                        std::sqrt(segment.length * segment.length + rise * rise));
                }
            }
            b_.defines(segmentKey + "/common", b_.propertySet(segmentKey + "/common", common, list),
                       {element});
        }
        if (run.element.entity == "IfcPipeSegment" && in3d && segment.length > 0.0) {
            PropertyList occurrence;
            occurrence.positiveRatio("Gradient",
                                     std::abs(*endCentre - *startCentre) / segment.length);
            if (service.diameterIsInside && size > 0.0) {
                // The inside bottom of the bore, at the start: centre less
                // half the inside size is exact for an inside size.
                occurrence.length("InvertElevation", *startCentre - size / 2.0);
            }
            b_.defines(segmentKey + "/occurrence",
                       b_.propertySet(segmentKey + "/occurrence", "Pset_PipeSegmentOccurrence",
                                      occurrence),
                       {element});
        }
        b_.associate(qualityReference(segment.level), element);
        return element;
    }

    Id writeFeature(const sub::UtilityLine& line, const UtilityClass& feature,
                    const std::string& key)
    {
        const sub::UtilityVertex& vertex = line.vertices.front();
        std::vector<Id> items{b_.point(planOf(vertex))};
        if (line.attributes.diameter > 0.0) {
            const Id placement = b_.placement2(b_.point(planOf(vertex)), 0.0);
            items.push_back(b_.file().add(
                "IfcCircle", Args().ref(placement).real(line.attributes.diameter / 2.0)));
        }
        const Id set = b_.file().add("IfcGeometricCurveSet", Args().refs(items));
        const Id representation =
            b_.shape(b_.footPrintContext(), "FootPrint", "GeometricCurveSet", {set});
        const auto classification = sub::classify(vertex.evidence, input_.grading.tolerances);
        b_.layer(std::string("Utilities/") + sub::toString(line.attributes.type) + "/" +
                     sub::toString(classification.level),
                 representation);
        const Id element = b_.product(feature.element, key + "/feature", line.id, feature.reason,
                                      b_.productShape({representation}), line.id);
        b_.tally("service " + line.id, feature.element,
                 feature.system == "USERDEFINED" ? feature.systemObjectType : feature.system,
                 "a service at one point: " + feature.reason);
        if (const std::string_view common = commonPropertySet(feature.element.entity);
            !common.empty()) {
            PropertyList list;
            list.identifier("Reference", line.id);
            list.enumerated("Status", elementStatus(line.attributes.status));
            b_.defines(key + "/feature/common",
                       b_.propertySet(key + "/feature/common", common, list), {element});
        }
        b_.associate(qualityReference(classification.level), element);
        return element;
    }

    Id writeLocatedPoint(const sub::UtilityLine& line, const std::optional<sub::GradedLine>& graded,
                         const std::vector<sub::CoverResult>& covers, std::size_t index,
                         const std::string& key)
    {
        const sub::UtilityVertex& vertex = line.vertices[index];
        const sub::Classification classification =
            graded ? graded->vertices[index].classification
                   : sub::classify(vertex.evidence, input_.grading.tolerances);
        const auto level = sub::serviceLevel(vertex);
        const Id point =
            level ? b_.point(Vec3(vertex.position.easting, vertex.position.northing, *level))
                  : b_.point(planOf(vertex));
        const Id representation = b_.shape(b_.annotationContext(), "Annotation", "Point", {point});
        b_.layer(std::string("Utilities/") + sub::toString(line.attributes.type) + "/points",
                 representation);
        const std::string pointKey = key + "/point/" + vertex.id;
        const Id annotation =
            b_.product(IfcClass{"IfcAnnotation", "SURVEY", {}}, pointKey, vertex.id,
                       std::string("Located by ") + sub::toString(vertex.evidence.method),
                       b_.productShape({representation}));
        ++b_.report().locatedPoints;
        // In the service's system, as the file groups it.
        const UtilityClass run = classifyUtilityRun(line.attributes);
        b_.tally("service " + line.id, IfcClass{"IfcAnnotation", "SURVEY", {}},
                 run.system == "USERDEFINED" ? run.systemObjectType : run.system,
                 "a located point");

        PropertyList located;
        located.label("LocateMethod", sub::toString(vertex.evidence.method));
        located.label("QualityLevel", sub::toString(classification.level),
                      "The AS 5488.1-2019 quality level the point's evidence supports");
        if (vertex.claimed) {
            located.label("QualityLevelClaimed", sub::toString(*vertex.claimed));
            located.boolean("ClaimSupported", *vertex.claimed <= classification.level);
        }
        if (graded) {
            located.text("OverClaim", graded->vertices[index].overClaim);
        }
        located.text("Reasons", joined(classification.reasons, "; "));
        if (vertex.evidence.hasLevel) {
            located.boolean("LevelQualified", classification.levelQualified);
        }
        located.label("LevelReference", level ? levelReferenceName(vertex.levelReference) : "");
        located.length("Level", vertex.level);
        located.length("SurfaceLevel", vertex.surfaceLevel);
        located.length("Depth", vertex.depth);
        located.length("TopLevel", sub::topLevel(vertex, line.attributes.diameter));
        if (index < covers.size()) {
            located.length("DepthOfCover", covers[index].cover);
            located.text("CoverNote", covers[index].note);
        }
        located.identifier("Verifies", vertex.verifies);
        located.label("Standard", kStandard);
        b_.defines(pointKey + "/located",
                   b_.propertySet(pointKey + "/located", "AS5488_LocatedPoint", located),
                   {annotation});

        PropertyList uncertainty;
        uncertainty.enumerated("UncertaintyBasis", uncertaintyBasis(classification.level));
        if (vertex.evidence.horizontalUncertainty || vertex.evidence.verticalUncertainty) {
            uncertainty.text("UncertaintyDescription",
                             "The assessed uncertainty of the located position");
        }
        uncertainty.positiveLength("HorizontalUncertainty", vertex.evidence.horizontalUncertainty);
        uncertainty.positiveLength("VerticalUncertainty", vertex.evidence.verticalUncertainty);
        b_.defines(pointKey + "/uncertainty",
                   b_.propertySet(pointKey + "/uncertainty", "Pset_Uncertainty", uncertainty),
                   {annotation});

        if (deliverySchedule(line)) {
            b_.defines(pointKey + "/delivery",
                       b_.propertySet(
                           pointKey + "/delivery", deliveryPropertySetName(input_.schema),
                           deliveryProperties(vertex.written, vertex.fields, kSchemaPointColumns)),
                       {annotation});
        }
        b_.associate(qualityReference(classification.level), annotation);
        return annotation;
    }

    [[nodiscard]] PropertyList serviceProperties(const sub::UtilityAttributes& service,
                                                 const std::optional<sub::GradedLine>& graded) const
    {
        PropertyList list;
        list.label("UtilityType", sub::toString(service.type));
        list.label("Owner", service.owner);
        list.label("Material", service.material);
        list.positiveLength("Size", service.diameter);
        if (service.diameter > 0.0) {
            list.boolean("SizeIsInside", service.diameterIsInside);
        }
        list.label("Configuration", service.configuration);
        list.label("Status", service.status == sub::UtilityStatus::Unknown
                                 ? ""
                                 : sub::toString(service.status));
        list.text("Description", service.description);
        if (graded) {
            list.length("PlanLength", graded->length());
            for (const sub::QualityLevel level : {sub::QualityLevel::A, sub::QualityLevel::B,
                                                  sub::QualityLevel::C, sub::QualityLevel::D}) {
                const double length = graded->lengthAt[static_cast<int>(level)];
                if (length > 0.0) {
                    list.length(std::string("LengthAtQL") + sub::toString(level)[3], length);
                }
            }
        }
        list.label("SourceSchedule", input_.sourceName);
        list.label("Standard", kStandard);
        return list;
    }

    // The delivery schema's attributes: those the reader interpreted, as
    // written, and those it carried by name.
    PropertyList deliveryProperties(const std::map<std::string, std::string>& written,
                                    const std::map<std::string, std::string>& fields,
                                    std::span<const std::string_view> interpreted)
    {
        std::map<std::string, std::string> values;
        for (const std::string_view column : interpreted) {
            if (const auto found = written.find(std::string(column)); found != written.end()) {
                values.emplace(found->first, found->second);
            }
        }
        for (const auto& [name, value] : fields) {
            values.emplace(name, value);
        }
        PropertyList list;
        const sub::DeliverySchema* schema = input_.schema;
        if (!schema) {
            for (const auto& [name, value] : values) {
                list.label(name, value);
            }
            return list;
        }
        // In the schema's order, described by its labels, enumerated where
        // it lists the values. A value its list does not hold (the check
        // reports it) is written as the text it is: an enumerated value
        // outside its enumeration is a schema error in the file (WR21 of
        // IfcPropertyEnumeratedValue), and the value must not be lost.
        for (const sub::SchemaField& field : schema->fields) {
            const auto found = values.find(field.attribute);
            if (found == values.end() || found->second.empty()) {
                continue;
            }
            const std::string& value = found->second;
            const auto domain = schema->domains.find(field.attribute);
            if (domain != schema->domains.end()) {
                const bool listed = std::any_of(
                    domain->second.values.begin(), domain->second.values.end(),
                    [&](const sub::SchemaValue& candidate) { return candidate.value == value; });
                if (listed) {
                    list.enumerated(field.attribute, value,
                                    enumerationOf(field.attribute, domain->second), field.label);
                    continue;
                }
            }
            if (field.type.starts_with("Real")) {
                if (const auto number = core::parseFiniteDouble(value)) {
                    list.add(
                        {field.attribute, typedValue("IfcReal", stepReal(*number)), field.label});
                    continue;
                }
            } else if (field.type.starts_with("Integer")) {
                if (const auto number = core::parseInteger(value)) {
                    list.add({field.attribute, typedValue("IfcInteger", std::to_string(*number)),
                              field.label});
                    continue;
                }
            }
            list.label(field.attribute, value, field.label);
        }
        return list;
    }

    Id enumerationOf(const std::string& attribute, const sub::SchemaDomain& domain)
    {
        std::vector<std::string> encoded;
        std::set<std::string> seen;
        for (const sub::SchemaValue& value : domain.values) {
            if (seen.insert(value.value).second) {
                encoded.push_back(typedValue("IfcLabel", stepString(value.value)));
            }
        }
        return b_.enumeration(attribute, encoded);
    }

    Builder& b_;
    const UtilityInput& input_;
    Id standard_ = 0;
    Id schemaClassification_ = 0;
};

} // namespace

void exportUtilities(Builder& builder, const UtilityInput& utilities)
{
    UtilityWriter(builder, utilities).write();
}

} // namespace katana::ifc::detail
