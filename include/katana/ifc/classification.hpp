#pragma once

// Which IFC 4.3 class each thing Katana exports becomes - the decision the
// export exists to get right. An IFC file whose every object is an
// IfcBuildingElementProxy says nothing a DXF did not: a consumer cannot ask it
// for the water mains, the kerbs or the pits. So nothing here falls back to a
// proxy. What is known to be an element becomes the element class the schema
// has for it; what is only known to be a line someone surveyed or drew
// becomes an IfcAnnotation (SURVEY, CONTOURLINE, TEXT, DIMENSION, LEADER),
// which is what IFC 4.3 made that class for - "annotations are also used to
// represent survey elements" - and which claims no more than is known.
//
// Two sources decide it:
//
//   * A SERVICE from an AS 5488 schedule (survey/subsurface): its type - the
//     AS 5488.2 Table A.4 asset type the TfNSW Utility Schema codes as
//     AssetTypeCode - gives the distribution system and the default segment
//     class, and the words of its AssetFeature, AssetSubtype, configuration,
//     material and description refine the class: a "Conduit (with Cable)" is
//     an IfcCableCarrierSegment, a "Culvert" an IfcPipeSegment CULVERT. A
//     service recorded at one point is a pit, a valve, a hydrant or a marker
//     when its feature says which.
//
//   * A DRAWING ENTITY: an ordered list of rules over the words of its layer
//     path, its survey code and its 12d string name - "KERB" is an IfcKerb,
//     "FENCE" an IfcRailing FENCE, a point on a "SEWER PITS" layer an
//     IfcDistributionChamberElement in the SEWAGE system. The first rule that
//     matches wins; the rules are data (ClassificationRule), so a project
//     whose layers are named otherwise passes its own.
//
// docs/ifc.md has the tables, and why each mapping was chosen.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/survey/subsurface/utility_network.hpp"

namespace katana::ifc {

// An IFC class and its predefined type, spelt as the schema spells them.
struct IfcClass {
    std::string entity;         // "IfcPipeSegment"
    std::string predefinedType; // "RIGIDSEGMENT"; empty when the class has none or it is not known
    std::string objectType;     // what a USERDEFINED one is ("HEADWALL"); empty otherwise

    friend bool operator==(const IfcClass&, const IfcClass&) = default;
};

// ---- services (AS 5488 / TfNSW) ----------------------------------------------

struct UtilityClass {
    IfcClass element;             // a run's segments, or a point feature
    std::string system;           // the IfcDistributionSystemEnum value: "WATERSUPPLY"
    std::string systemObjectType; // what a USERDEFINED system is: "RECYCLEDWATER"
    // What decided the element class, for the export's report and for a
    // person asking why: "AssetFeature \"Conduit (with Cable)\"", "type water".
    std::string reason;
};

// The class of every segment of a service run:
//
//   water, sewer, stormwater, gas, fuel, fire service, recycled water
//                               IfcPipeSegment RIGIDSEGMENT
//   electricity, communications, ITS
//                               IfcCableSegment CABLESEGMENT
//   a culvert                   IfcPipeSegment CULVERT
//   a conduit or duct           IfcCableCarrierSegment CONDUITSEGMENT
//   a trough or trunking        IfcCableCarrierSegment CABLETRUNKINGSEGMENT
//   optical fibre               IfcCableSegment OPTICALCABLESEGMENT
//   a service of unknown kind   IfcDistributionFlowElement - the schema's
//                               element of a distribution flow system whose
//                               kind is not said, which is exactly what
//                               AS 5488 calls a service of unknown type
//                               (IfcFlowSegment would say it too, but IFC 4.3
//                               deprecates instantiating it)
//
// and the system: WATERSUPPLY, SEWAGE, STORMWATER, GAS, FUEL, FIREPROTECTION,
// ELECTRICAL, COMMUNICATION, CONTROL (ITS: traffic signals and their
// detection), USERDEFINED RECYCLEDWATER (IFC has no recycled water system),
// NOTDEFINED for an unknown service.
[[nodiscard]] UtilityClass classifyUtilityRun(const survey::subsurface::UtilityAttributes& service);

// A service recorded at a single point: a manhole, pit, chamber or sump
// (IfcDistributionChamberElement), a hydrant (IfcFireSuppressionTerminal
// FIREHYDRANT), a valve (IfcValve), a marker post (IfcSign MARKER), a pillar
// or cabinet (IfcJunctionBox POWER or DATA). nullopt when its words name none
// of them: the point is then exported as the survey record it is and nothing
// more.
[[nodiscard]] std::optional<UtilityClass>
classifyUtilityPoint(const survey::subsurface::UtilityAttributes& service);

// The one-letter asset type code of AS 5488.2 Table A.4 that the TfNSW
// Utility Schema's AssetTypeCode takes (C D E F G I P S W N), for a type the
// table lists; empty for recycled water and "other", which it does not.
[[nodiscard]] std::string_view assetTypeCode(survey::subsurface::UtilityType type);

// ---- drawing entities --------------------------------------------------------

// Words are matched whole and without regard to case against the entity's
// classificationText: "KERB" matches the layer "Survey/Kerb" and the code
// "KERB" but not "KERBSIDE"; a word ending in '*' matches as a prefix, so
// "ELEC*" matches "ELECTRICAL" and "ELEC". A word of several words ("EDGE OF
// SEAL") matches them in sequence.
struct ClassificationRule {
    std::string name;               // shown in the report: "kerb"
    std::vector<std::string> words; // any one of them
    // The entity kinds the rule applies to; empty for any. A point on a
    // "PITS" layer is a pit; a line on it is not.
    std::vector<entity::EntityType> kinds;
    IfcClass target;
    // The distribution system a service element belongs to, when the rule
    // itself says ("SEWER" lines are in SEWAGE). Empty: taken from the
    // service words of the same text (serviceSystemFor), so a pit on a
    // "STORMWATER PITS" layer is in STORMWATER without a rule per service.
    std::string system;
};

// Services first (a "SEWER" polyline is a pipe, whatever else its layer
// says), then point features, then the road furniture and contours; in that
// order, first match wins. The list is in the header of classification.cpp
// with the reason for each entry, and in docs/ifc.md.
[[nodiscard]] const std::vector<ClassificationRule>& defaultClassificationRules();

// What an entity is classified by, upper case, words separated by single
// blanks: its layer path, its survey code (the "code" property), and the
// name 12d gave the string ("12d.name"). "Survey/Kerb" with code "KB01" is
// "SURVEY KERB KB01".
[[nodiscard]] std::string classificationText(const entity::Entity& entity);

// True when `text` (as classificationText makes it) holds `word` under the
// matching rules above.
[[nodiscard]] bool containsWord(std::string_view text, std::string_view word);

// The distribution system (IfcDistributionSystemEnum) a text's service words
// name - STORMWATER for "STORM", "DRAINAGE" or "SW"; SEWAGE, WATERSUPPLY, GAS,
// FUEL, ELECTRICAL, COMMUNICATION - or empty for none.
[[nodiscard]] std::string serviceSystemFor(std::string_view text);

// A project's rules as a file, so that layers named otherwise than the
// defaults expect are classified as the project means them - the way a
// wrong class shown in the window's preview is put right. CSV, one rule a
// row, the columns found by name in a header row, in any order and letter
// case; '#' starts a comment line:
//
//   rule        the rule's name, shown in reports                   required
//   words       its words, separated by ';' ("POLE*;LP")             required
//   kinds       Point;Line;Arc;Polyline;Circle;Text;Dimension;
//               Label;Leader - empty for any kind
//   class       the IFC class, as the schema spells it ("IfcColumn")  required
//   predefined_type   "COLUMN"; empty when the class has none
//   object_type what a USERDEFINED one is; required with USERDEFINED
//   system      the distribution system its elements serve ("SEWAGE")
//
// ParseFailure naming the line for a class this export does not write, a
// predefined type on a class without one, a kind that is not one, a rule
// with no words, and USERDEFINED without an object type - the same things
// writeIfc refuses, found when the file is read rather than when it is used.
[[nodiscard]] core::Result<std::vector<ClassificationRule>>
parseClassificationRules(std::string_view text);

// `rules` as parseClassificationRules reads them: what the window writes as
// a starting point for a project's own file (the defaults, commented).
[[nodiscard]] std::string formatClassificationRules(const std::vector<ClassificationRule>& rules);

struct EntityClass {
    IfcClass ifcClass;
    std::string system; // for a distribution element; empty otherwise
    std::string rule;   // the rule that matched; empty for the fallback
};

// The first rule that matches, else by the entity's kind: a point or a line
// someone surveyed (it has heights or a survey code) is an IfcAnnotation
// SURVEY; other drawn linework an IfcAnnotation NOTDEFINED; text TEXT; a
// dimension DIMENSION; a leader LEADER. A contour-line rule makes
// CONTOURLINE.
[[nodiscard]] EntityClass classifyEntity(const entity::Entity& entity,
                                         const std::vector<ClassificationRule>& rules);

} // namespace katana::ifc
