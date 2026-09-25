#include "katana/ifc/classification.hpp"

#include <algorithm>
#include <array>
#include <cctype>

#include "katana/core/text.hpp"

namespace katana::ifc {

namespace sub = katana::survey::subsurface;
using entity::EntityType;

namespace {

// Upper case, every run of characters that are not letters or digits one
// blank, no blank at either end: "Survey/Kerb_line" is "SURVEY KERB LINE".
std::string normalised(std::string_view text)
{
    std::string out;
    out.reserve(text.size());
    bool gap = false;
    for (const char raw : text) {
        const auto c = static_cast<unsigned char>(raw);
        if (std::isalnum(c) != 0 || c >= 0x80) {
            if (gap && !out.empty()) {
                out.push_back(' ');
            }
            gap = false;
            out.push_back(static_cast<char>(std::toupper(c)));
        } else {
            gap = true;
        }
    }
    return out;
}

std::string propertyText(const entity::PropertyMap& map, std::string_view key)
{
    const auto found = map.find(key);
    if (found == map.end()) {
        return {};
    }
    return entity::toString(found->second);
}

// The distribution systems, by the service words that name them. Longer and
// more specific words first: "RECYCLED WATER" before "WATER", and
// "STORMWATER" is one word, so it never reads as WATER.
struct ServiceWords {
    std::string_view system;
    std::string_view objectType; // for a USERDEFINED system
    std::array<std::string_view, 8> words;
};

constexpr std::array<ServiceWords, 9> kServices{{
    {"USERDEFINED", "RECYCLEDWATER", {"RECYCLED WATER", "RECYCLED", "REUSE", "PURPLE PIPE"}},
    {"FIREPROTECTION", "", {"FIRE SERVICE", "FIRE MAIN", "HYDRANT", "HYDRANTS"}},
    {"STORMWATER", "",
     {"STORMWATER", "STORM", "DRAINAGE", "DRAIN", "DRAINS", "SW", "CULVERT", "CULVERTS"}},
    {"SEWAGE", "", {"SEWER", "SEWERAGE", "SEWAGE", "SEWERS", "WASTEWATER"}},
    {"WATERSUPPLY", "", {"WATER", "POTABLE", "WATERMAIN", "WATERMAINS"}},
    {"GAS", "", {"GAS"}},
    {"FUEL", "", {"FUEL", "PETROLEUM"}},
    {"ELECTRICAL", "", {"ELEC*", "POWER", "HV", "LV", "STREETLIGHT*"}},
    {"COMMUNICATION", "",
     {"COMMS", "TELECOM*", "TELSTRA", "NBN", "OPTIC*", "FIBRE", "COMMUNICATION*", "TELCO"}},
}};

// Whole-word search in a normalised text for a normalised word or phrase.
bool hasWord(std::string_view text, std::string_view word)
{
    if (word.empty()) {
        return false;
    }
    const bool prefix = word.back() == '*';
    const std::string needle = normalised(prefix ? word.substr(0, word.size() - 1) : word);
    if (needle.empty()) {
        return false;
    }
    std::size_t at = 0;
    while ((at = text.find(needle, at)) != std::string_view::npos) {
        const bool startsWord = at == 0 || text[at - 1] == ' ';
        const std::size_t end = at + needle.size();
        const bool endsWord = end == text.size() || text[end] == ' ';
        if (startsWord && (endsWord || prefix)) {
            return true;
        }
        ++at;
    }
    return false;
}

bool hasAny(std::string_view text, std::initializer_list<std::string_view> words)
{
    return std::any_of(words.begin(), words.end(),
                       [&](std::string_view word) { return hasWord(text, word); });
}

const std::string* fieldOf(const sub::UtilityAttributes& service, std::string_view name)
{
    const auto found = service.fields.find(std::string(name));
    return found == service.fields.end() ? nullptr : &found->second;
}

// Everything a service says about what it is, normalised, AssetFeature first.
std::string featureText(const sub::UtilityAttributes& service)
{
    std::string text;
    const auto append = [&](std::string_view part) {
        if (!part.empty()) {
            text += ' ';
            text += part;
        }
    };
    for (const std::string_view name :
         {"AssetFeature", "AssetFeatureDescription", "AssetSubtype", "AssetSubtypeDescription"}) {
        if (const std::string* value = fieldOf(service, name)) {
            append(*value);
        }
    }
    append(service.configuration);
    append(service.material);
    append(service.description);
    return normalised(text);
}

// Which of the feature's fields holds `word`, for the reason given: the
// schema's own attribute name where it came from one.
std::string reasonFor(const sub::UtilityAttributes& service, std::string_view word)
{
    for (const std::string_view name :
         {"AssetFeature", "AssetFeatureDescription", "AssetSubtype", "AssetSubtypeDescription"}) {
        if (const std::string* value = fieldOf(service, name);
            value && hasWord(normalised(*value), word)) {
            return std::string(name) + " \"" + *value + "\"";
        }
    }
    for (const auto& [name, value] :
         {std::pair<std::string_view, const std::string*>{"configuration", &service.configuration},
          {"material", &service.material},
          {"description", &service.description}}) {
        if (hasWord(normalised(*value), word)) {
            return std::string(name) + " \"" + *value + "\"";
        }
    }
    return std::string(word);
}

bool carriesCable(sub::UtilityType type)
{
    return type == sub::UtilityType::Electricity ||
           type == sub::UtilityType::Telecommunications ||
           type == sub::UtilityType::IntelligentTransport;
}

bool carriesFluid(sub::UtilityType type)
{
    switch (type) {
    case sub::UtilityType::Water:
    case sub::UtilityType::RecycledWater:
    case sub::UtilityType::FireService:
    case sub::UtilityType::Sewer:
    case sub::UtilityType::Stormwater:
    case sub::UtilityType::Gas:
    case sub::UtilityType::Fuel:
        return true;
    default:
        return false;
    }
}

std::pair<std::string, std::string> systemOf(sub::UtilityType type)
{
    switch (type) {
    case sub::UtilityType::Water:
        return {"WATERSUPPLY", ""};
    case sub::UtilityType::RecycledWater:
        return {"USERDEFINED", "RECYCLEDWATER"};
    case sub::UtilityType::FireService:
        return {"FIREPROTECTION", ""};
    case sub::UtilityType::Sewer:
        return {"SEWAGE", ""};
    case sub::UtilityType::Stormwater:
        return {"STORMWATER", ""};
    case sub::UtilityType::Gas:
        return {"GAS", ""};
    case sub::UtilityType::Fuel:
        return {"FUEL", ""};
    case sub::UtilityType::Electricity:
        return {"ELECTRICAL", ""};
    case sub::UtilityType::Telecommunications:
        return {"COMMUNICATION", ""};
    case sub::UtilityType::IntelligentTransport:
        // Traffic signals, their detector loops and cameras: the control of
        // traffic. IFC's SIGNAL is railway signalling.
        return {"CONTROL", ""};
    case sub::UtilityType::Unknown:
    case sub::UtilityType::Other:
        break;
    }
    return {"NOTDEFINED", ""};
}

} // namespace

// ---- services -------------------------------------------------------------------

UtilityClass classifyUtilityRun(const sub::UtilityAttributes& service)
{
    UtilityClass result;
    std::tie(result.system, result.systemObjectType) = systemOf(service.type);
    const std::string text = featureText(service);
    const auto set = [&](std::string_view entity, std::string_view predefined,
                         std::string_view word) {
        result.element = IfcClass{std::string(entity), std::string(predefined), {}};
        result.reason = reasonFor(service, word);
    };

    // What the service is laid in, or as, decides the class before its
    // type does: a culvert is a culvert whatever it drains, and a conduit is
    // a cable carrier whatever it carries.
    if (hasWord(text, "CULVERT*")) {
        set("IfcPipeSegment", "CULVERT", "CULVERT*");
        return result;
    }
    if (!carriesFluid(service.type)) {
        if (hasAny(text, {"CONDUIT*", "DUCT*"})) {
            set("IfcCableCarrierSegment", "CONDUITSEGMENT",
                hasWord(text, "CONDUIT*") ? "CONDUIT*" : "DUCT*");
            return result;
        }
        if (hasAny(text, {"TROUGH*", "TRUNKING"})) {
            set("IfcCableCarrierSegment", "CABLETRUNKINGSEGMENT",
                hasWord(text, "TROUGH*") ? "TROUGH*" : "TRUNKING");
            return result;
        }
        if (hasAny(text, {"OPTIC*", "FIBRE*", "FIBER*"})) {
            set("IfcCableSegment", "OPTICALCABLESEGMENT",
                hasWord(text, "OPTIC*") ? "OPTIC*" : (hasWord(text, "FIBRE*") ? "FIBRE*" : "FIBER*"));
            return result;
        }
        if (hasWord(text, "CABLE*")) {
            set("IfcCableSegment", "CABLESEGMENT", "CABLE*");
            return result;
        }
    }
    if (carriesCable(service.type)) {
        result.element = IfcClass{"IfcCableSegment", "CABLESEGMENT", {}};
        result.reason = std::string("type ") + sub::toString(service.type);
        return result;
    }
    if (carriesFluid(service.type)) {
        result.element = IfcClass{"IfcPipeSegment", "RIGIDSEGMENT", {}};
        result.reason = std::string("type ") + sub::toString(service.type);
        return result;
    }
    if (hasAny(text, {"PIPE*", "MAIN", "MAINS"})) {
        set("IfcPipeSegment", "RIGIDSEGMENT", hasWord(text, "PIPE*") ? "PIPE*" : "MAIN*");
        return result;
    }
    // A service whose kind nobody established: the schema's element of a
    // distribution flow system, which says exactly that and no more. (Not
    // IfcFlowSegment, whose instantiation IFC 4.3 deprecates - IFC102.)
    result.element = IfcClass{"IfcDistributionFlowElement", {}, "UNKNOWN SERVICE"};
    result.reason = std::string("type ") + sub::toString(service.type);
    return result;
}

std::optional<UtilityClass> classifyUtilityPoint(const sub::UtilityAttributes& service)
{
    UtilityClass result;
    std::tie(result.system, result.systemObjectType) = systemOf(service.type);
    const std::string text = featureText(service);
    const auto make = [&](std::string_view entity, std::string_view predefined,
                          std::string_view word) {
        result.element = IfcClass{std::string(entity), std::string(predefined), {}};
        result.reason = reasonFor(service, word);
        return result;
    };
    // Most specific first: a "valve pit" is a chamber, not a valve.
    if (hasWord(text, "MANHOLE*")) {
        return make("IfcDistributionChamberElement", "MANHOLE", "MANHOLE*");
    }
    if (hasWord(text, "VALVE PIT") || hasWord(text, "VALVE CHAMBER")) {
        return make("IfcDistributionChamberElement", "VALVECHAMBER",
                    hasWord(text, "VALVE PIT") ? "VALVE PIT" : "VALVE CHAMBER");
    }
    if (hasWord(text, "METER PIT") || hasWord(text, "METER CHAMBER")) {
        return make("IfcDistributionChamberElement", "METERCHAMBER",
                    hasWord(text, "METER PIT") ? "METER PIT" : "METER CHAMBER");
    }
    if (hasWord(text, "SUMP*")) {
        return make("IfcDistributionChamberElement", "SUMP", "SUMP*");
    }
    if (hasWord(text, "CHAMBER*")) {
        return make("IfcDistributionChamberElement", "INSPECTIONCHAMBER", "CHAMBER*");
    }
    if (hasWord(text, "PIT") || hasWord(text, "PITS")) {
        return make("IfcDistributionChamberElement", "INSPECTIONPIT",
                    hasWord(text, "PIT") ? "PIT" : "PITS");
    }
    if (hasWord(text, "HYDRANT*")) {
        return make("IfcFireSuppressionTerminal", "FIREHYDRANT", "HYDRANT*");
    }
    if (hasWord(text, "STOP VALVE") || hasWord(text, "STOPCOCK")) {
        return make("IfcValve", "STOPCOCK", hasWord(text, "STOPCOCK") ? "STOPCOCK" : "STOP VALVE");
    }
    if (hasWord(text, "AIR VALVE")) {
        return make("IfcValve", "AIRRELEASE", "AIR VALVE");
    }
    if (hasWord(text, "SCOUR*")) {
        return make("IfcValve", "FLUSHING", "SCOUR*");
    }
    if (hasWord(text, "VALVE*")) {
        return make("IfcValve", "ISOLATING", "VALVE*");
    }
    if (hasWord(text, "MARKER*")) {
        return make("IfcSign", "MARKER", "MARKER*");
    }
    if (hasAny(text, {"PILLAR*", "CABINET*"})) {
        return make("IfcJunctionBox",
                    service.type == sub::UtilityType::Electricity ? "POWER" : "DATA",
                    hasWord(text, "PILLAR*") ? "PILLAR*" : "CABINET*");
    }
    return std::nullopt;
}

std::string_view assetTypeCode(sub::UtilityType type)
{
    switch (type) {
    case sub::UtilityType::Telecommunications:
        return "C";
    case sub::UtilityType::Stormwater:
        return "D";
    case sub::UtilityType::Electricity:
        return "E";
    case sub::UtilityType::FireService:
        return "F";
    case sub::UtilityType::Gas:
        return "G";
    case sub::UtilityType::IntelligentTransport:
        return "I";
    case sub::UtilityType::Fuel:
        return "P";
    case sub::UtilityType::Sewer:
        return "S";
    case sub::UtilityType::Water:
        return "W";
    case sub::UtilityType::Unknown:
        return "N";
    case sub::UtilityType::RecycledWater:
    case sub::UtilityType::Other:
        break;
    }
    return {};
}

// ---- drawing entities ---------------------------------------------------------------

namespace {

// A run of something - a pipe, a kerb, a fence - is a line, an arc or a
// polyline; a circle on a pits layer is a pit's outline, not a pipe.
const std::vector<EntityType> kRuns{EntityType::Line, EntityType::Arc, EntityType::Polyline};
const std::vector<EntityType> kPoints{EntityType::Point};
const std::vector<EntityType> kPointsAndOutlines{EntityType::Point, EntityType::Circle};

ClassificationRule rule(std::string name, std::vector<std::string> words,
                        std::vector<EntityType> kinds, IfcClass target, std::string system = {})
{
    return ClassificationRule{std::move(name), std::move(words), std::move(kinds),
                              std::move(target), std::move(system)};
}

} // namespace

const std::vector<ClassificationRule>& defaultClassificationRules()
{
    // Services first: linework on a service's layer is the service. Its
    // system comes from the same words (serviceSystemFor), so the rules
    // name only the class. A culvert before a pipe, conduits before cables.
    static const std::vector<ClassificationRule> kRules{
        rule("culvert", {"CULVERT", "CULVERTS", "BOX CULVERT"}, kRuns,
             {"IfcPipeSegment", "CULVERT", {}}),
        rule("conduit", {"CONDUIT*", "DUCT", "DUCTS"}, kRuns,
             {"IfcCableCarrierSegment", "CONDUITSEGMENT", {}}),
        rule("fluid service",
             {"STORMWATER", "STORM", "DRAINAGE", "DRAIN", "DRAINS", "SW", "SEWER", "SEWERAGE",
              "SEWAGE", "WASTEWATER", "WATER", "WATERMAIN", "WATERMAINS", "POTABLE", "RECYCLED",
              "GAS", "FUEL", "FIRE MAIN"},
             kRuns, {"IfcPipeSegment", "RIGIDSEGMENT", {}}),
        rule("optical cable", {"OPTIC*", "FIBRE", "FIBRE OPTIC"}, kRuns,
             {"IfcCableSegment", "OPTICALCABLESEGMENT", {}}),
        rule("cable",
             {"ELEC*", "POWER", "HV", "LV", "COMMS", "TELECOM*", "TELSTRA", "NBN", "TELCO",
              "COMMUNICATION*", "CABLE*"},
             kRuns, {"IfcCableSegment", "CABLESEGMENT", {}}),

        // Features of a service, surveyed as points.
        rule("manhole", {"MANHOLE*", "MH"}, kPointsAndOutlines,
             {"IfcDistributionChamberElement", "MANHOLE", {}}),
        rule("sump", {"SUMP*"}, kPointsAndOutlines, {"IfcDistributionChamberElement", "SUMP", {}}),
        rule("chamber", {"CHAMBER*", "ACCESS CHAMBER"}, kPointsAndOutlines,
             {"IfcDistributionChamberElement", "INSPECTIONCHAMBER", {}}),
        rule("pit", {"PIT", "PITS", "SIP", "GULLY", "GULLIES", "KIP"}, kPointsAndOutlines,
             {"IfcDistributionChamberElement", "INSPECTIONPIT", {}}),
        rule("hydrant", {"HYDRANT*", "FH"}, kPoints,
             {"IfcFireSuppressionTerminal", "FIREHYDRANT", {}}, "FIREPROTECTION"),
        rule("valve", {"VALVE*", "SV", "WV", "GV"}, kPoints, {"IfcValve", "ISOLATING", {}}),
        rule("marker", {"MARKER*"}, kPoints, {"IfcSign", "MARKER", {}}),

        // Road furniture and the ground.
        rule("kerb", {"KERB*", "KB", "EDGE OF KERB", "LIP OF KERB", "BACK OF KERB", "KERB AND GUTTER"},
             kRuns, {"IfcKerb", "NOTDEFINED", {}}),
        rule("guardrail", {"GUARDRAIL*", "GUARD RAIL", "BARRIER*", "W BEAM", "WBEAM", "THRIE BEAM"},
             kRuns, {"IfcRailing", "GUARDRAIL", {}}),
        rule("handrail", {"HANDRAIL*", "BALUSTRADE*"}, kRuns, {"IfcRailing", "HANDRAIL", {}}),
        rule("fence", {"FENCE*", "FENCING"}, kRuns, {"IfcRailing", "FENCE", {}}),
        rule("retaining wall", {"RETAINING WALL", "RETAINING"}, kRuns,
             {"IfcWall", "RETAININGWALL", {}}),
        rule("wall", {"WALL", "WALLS"}, kRuns, {"IfcWall", "NOTDEFINED", {}}),
        rule("vegetation", {"TREE", "TREES", "SHRUB*", "VEGETATION", "HEDGE*"}, {},
             {"IfcGeographicElement", "VEGETATION", {}}),
        // IfcBorehole, IFC 4.3's geotechnical borehole: the value
        // SOIL_BORING_POINT of IfcGeographicElement is deprecated (IFC102).
        rule("borehole", {"BOREHOLE*", "BH", "TEST PIT"}, kPoints, {"IfcBorehole", {}, {}}),
        rule("sign", {"SIGN", "SIGNS"}, kPoints, {"IfcSign", "NOTDEFINED", {}}),
        rule("contour", {"CONTOUR*"}, kRuns, {"IfcAnnotation", "CONTOURLINE", {}}),
    };
    return kRules;
}

std::string classificationText(const entity::Entity& entity)
{
    std::string text = entity.layer;
    for (const std::string& part :
         {propertyText(entity.properties, "code"), propertyText(entity.metadata, "12d.name")}) {
        if (!part.empty()) {
            text += ' ';
            text += part;
        }
    }
    return normalised(text);
}

bool containsWord(std::string_view text, std::string_view word)
{
    return hasWord(text, word);
}

std::string serviceSystemFor(std::string_view text)
{
    for (const ServiceWords& service : kServices) {
        for (const std::string_view word : service.words) {
            if (hasWord(text, word)) {
                // A system IFC has no value for is named by what it is,
                // and written USERDEFINED with that as its object type.
                return service.system == "USERDEFINED" ? std::string(service.objectType)
                                                       : std::string(service.system);
            }
        }
    }
    return {};
}

EntityClass classifyEntity(const entity::Entity& entity,
                           const std::vector<ClassificationRule>& rules)
{
    const EntityType kind = entity.type();
    const std::string text = classificationText(entity);
    for (const ClassificationRule& candidate : rules) {
        if (!candidate.kinds.empty() &&
            std::find(candidate.kinds.begin(), candidate.kinds.end(), kind) ==
                candidate.kinds.end()) {
            continue;
        }
        const bool matches =
            std::any_of(candidate.words.begin(), candidate.words.end(),
                        [&](const std::string& word) { return hasWord(text, word); });
        if (!matches) {
            continue;
        }
        EntityClass result{candidate.target, candidate.system, candidate.name};
        if (result.system.empty() && result.ifcClass.entity != "IfcAnnotation" &&
            result.ifcClass.entity != "IfcGeographicElement" &&
            result.ifcClass.entity != "IfcKerb" && result.ifcClass.entity != "IfcRailing" &&
            result.ifcClass.entity != "IfcWall" && result.ifcClass.entity != "IfcSign") {
            result.system = serviceSystemFor(text);
        }
        return result;
    }

    // The fallback, by kind: never a proxy.
    EntityClass result;
    result.ifcClass.entity = "IfcAnnotation";
    switch (kind) {
    case EntityType::Text:
    case EntityType::Label:
        result.ifcClass.predefinedType = "TEXT";
        break;
    case EntityType::Dimension:
        result.ifcClass.predefinedType = "DIMENSION";
        break;
    case EntityType::Leader:
        result.ifcClass.predefinedType = "LEADER";
        break;
    case EntityType::Point:
        result.ifcClass.predefinedType = "SURVEY";
        break;
    case EntityType::Line:
    case EntityType::Arc:
    case EntityType::Polyline:
    case EntityType::Circle: {
        const bool surveyed = entity.properties.contains(entity::kElevationProperty) ||
                              entity.properties.contains(entity::kElevationsProperty) ||
                              entity.properties.contains("code");
        result.ifcClass.predefinedType = surveyed ? "SURVEY" : "NOTDEFINED";
        break;
    }
    }
    return result;
}

} // namespace katana::ifc
