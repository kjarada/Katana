#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/ifc/classification.hpp"

namespace ifc = katana::ifc;
namespace sub = katana::survey::subsurface;
using katana::entity::Entity;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

sub::UtilityAttributes service(sub::UtilityType type, std::string configuration = {},
                               std::string material = {}, std::string description = {})
{
    sub::UtilityAttributes out;
    out.type = type;
    out.configuration = std::move(configuration);
    out.material = std::move(material);
    out.description = std::move(description);
    return out;
}

sub::UtilityAttributes withFeature(sub::UtilityType type, std::string feature)
{
    sub::UtilityAttributes out;
    out.type = type;
    out.fields["AssetFeature"] = std::move(feature);
    return out;
}

Entity point(std::string layer, std::string code = {})
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{Point2{0.0, 0.0}};
    entity.layer = std::move(layer);
    if (!code.empty()) {
        entity.properties["code"] = std::move(code);
    }
    return entity;
}

Entity polyline(std::string layer)
{
    Entity entity;
    entity.geometry = Polyline2{{Point2{0.0, 0.0}, Point2{10.0, 0.0}, Point2{20.0, 5.0}}, false};
    entity.layer = std::move(layer);
    return entity;
}

ifc::IfcClass cls(std::string entity, std::string predefined = {}, std::string objectType = {})
{
    return ifc::IfcClass{std::move(entity), std::move(predefined), std::move(objectType)};
}

} // namespace

// ---- services ---------------------------------------------------------------------

// The type alone decides a run's class when nothing says what it is laid in:
// AS 5488.2 Table A.4's fluid services are pipes, its cable services cables.
TEST(IfcUtilityClass, ARunIsAPipeOrACableByItsTypeAndItsSystemIsTheType)
{
    const auto water = ifc::classifyUtilityRun(service(sub::UtilityType::Water));
    EXPECT_EQ(water.element, cls("IfcPipeSegment", "RIGIDSEGMENT"));
    EXPECT_EQ(water.system, "WATERSUPPLY");
    EXPECT_EQ(water.reason, "type water");

    const auto sewer = ifc::classifyUtilityRun(service(sub::UtilityType::Sewer));
    EXPECT_EQ(sewer.element, cls("IfcPipeSegment", "RIGIDSEGMENT"));
    EXPECT_EQ(sewer.system, "SEWAGE");

    const auto telco = ifc::classifyUtilityRun(service(sub::UtilityType::Telecommunications));
    EXPECT_EQ(telco.element, cls("IfcCableSegment", "CABLESEGMENT"));
    EXPECT_EQ(telco.system, "COMMUNICATION");

    // Traffic signals are the control of traffic; IFC's SIGNAL is railway
    // signalling.
    EXPECT_EQ(ifc::classifyUtilityRun(service(sub::UtilityType::IntelligentTransport)).system,
              "CONTROL");
    EXPECT_EQ(ifc::classifyUtilityRun(service(sub::UtilityType::FireService)).system,
              "FIREPROTECTION");
    // IFC has no recycled water system: USERDEFINED, said what it is.
    const auto recycled = ifc::classifyUtilityRun(service(sub::UtilityType::RecycledWater));
    EXPECT_EQ(recycled.system, "USERDEFINED");
    EXPECT_EQ(recycled.systemObjectType, "RECYCLEDWATER");
    EXPECT_EQ(recycled.element, cls("IfcPipeSegment", "RIGIDSEGMENT"));
}

// What a service is laid in decides before what it carries: the sample
// schedule's E1 is "4 x 100 mm conduits", and a consumer asking for the
// conduits must find them.
TEST(IfcUtilityClass, ConduitsCulvertsTroughsAndFibreAreTheClassesThatSayThem)
{
    const auto conduits = ifc::classifyUtilityRun(
        service(sub::UtilityType::Electricity, "4 x 100 mm conduits", "PVC conduits"));
    EXPECT_EQ(conduits.element, cls("IfcCableCarrierSegment", "CONDUITSEGMENT"));
    EXPECT_EQ(conduits.system, "ELECTRICAL");
    EXPECT_EQ(conduits.reason, "configuration \"4 x 100 mm conduits\"");

    const auto culvert =
        ifc::classifyUtilityRun(withFeature(sub::UtilityType::Stormwater, "Box Culvert"));
    EXPECT_EQ(culvert.element, cls("IfcPipeSegment", "CULVERT"));
    EXPECT_EQ(culvert.system, "STORMWATER");
    EXPECT_EQ(culvert.reason, "AssetFeature \"Box Culvert\"");

    EXPECT_EQ(
        ifc::classifyUtilityRun(withFeature(sub::UtilityType::Electricity, "Cable Trough")).element,
        cls("IfcCableCarrierSegment", "CABLETRUNKINGSEGMENT"));
    EXPECT_EQ(ifc::classifyUtilityRun(
                  service(sub::UtilityType::Telecommunications, {}, {}, "optic fibre 144 core"))
                  .element,
              cls("IfcCableSegment", "OPTICALCABLESEGMENT"));
    // A gas main in a sleeve described as a duct is still the gas main: a
    // fluid service is never a cable carrier.
    EXPECT_EQ(ifc::classifyUtilityRun(service(sub::UtilityType::Gas, "in duct")).element,
              cls("IfcPipeSegment", "RIGIDSEGMENT"));
    // Whole words: "conductor" is not a conduit.
    EXPECT_EQ(
        ifc::classifyUtilityRun(service(sub::UtilityType::Electricity, {}, "copper conductor"))
            .element,
        cls("IfcCableSegment", "CABLESEGMENT"));
}

// AS 5488's service of unknown type is exactly IfcDistributionFlowElement's
// "an element of a distribution system, kind not said" - and not a proxy.
TEST(IfcUtilityClass, AServiceOfUnknownKindIsADistributionFlowElementUnlessItsWordsSayPipe)
{
    const auto unknown = ifc::classifyUtilityRun(service(sub::UtilityType::Unknown));
    EXPECT_EQ(unknown.element, cls("IfcDistributionFlowElement", "", "UNKNOWN SERVICE"));
    EXPECT_EQ(unknown.system, "NOTDEFINED");
    EXPECT_EQ(
        ifc::classifyUtilityRun(service(sub::UtilityType::Unknown, {}, {}, "old pipe")).element,
        cls("IfcPipeSegment", "RIGIDSEGMENT"));
}

TEST(IfcUtilityClass, APointServiceIsThePitValveHydrantOrMarkerItsWordsName)
{
    const auto at = [](sub::UtilityType type, const char* feature) {
        const auto result = ifc::classifyUtilityPoint(withFeature(type, feature));
        return result ? result->element : cls("none");
    };
    // Most specific first: a valve pit is a chamber, not a valve.
    EXPECT_EQ(at(sub::UtilityType::Water, "Valve Pit"),
              cls("IfcDistributionChamberElement", "VALVECHAMBER"));
    EXPECT_EQ(at(sub::UtilityType::Water, "Meter pit"),
              cls("IfcDistributionChamberElement", "METERCHAMBER"));
    EXPECT_EQ(at(sub::UtilityType::Sewer, "Manhole"),
              cls("IfcDistributionChamberElement", "MANHOLE"));
    EXPECT_EQ(at(sub::UtilityType::Telecommunications, "Pit"),
              cls("IfcDistributionChamberElement", "INSPECTIONPIT"));
    EXPECT_EQ(at(sub::UtilityType::Water, "Hydrant"),
              cls("IfcFireSuppressionTerminal", "FIREHYDRANT"));
    EXPECT_EQ(at(sub::UtilityType::Water, "Air Valve"), cls("IfcValve", "AIRRELEASE"));
    EXPECT_EQ(at(sub::UtilityType::Water, "Scour"), cls("IfcValve", "FLUSHING"));
    EXPECT_EQ(at(sub::UtilityType::Water, "Stop valve"), cls("IfcValve", "STOPCOCK"));
    EXPECT_EQ(at(sub::UtilityType::Gas, "Valve"), cls("IfcValve", "ISOLATING"));
    EXPECT_EQ(at(sub::UtilityType::Gas, "Marker post"), cls("IfcSign", "MARKER"));
    EXPECT_EQ(at(sub::UtilityType::Electricity, "Pillar"), cls("IfcJunctionBox", "POWER"));
    EXPECT_EQ(at(sub::UtilityType::Telecommunications, "Cabinet"), cls("IfcJunctionBox", "DATA"));
    // Nothing named: no class is claimed, and the point stays the survey
    // record it is.
    EXPECT_FALSE(ifc::classifyUtilityPoint(service(sub::UtilityType::Water)));
    EXPECT_FALSE(ifc::classifyUtilityPoint(withFeature(sub::UtilityType::Water, "Pipe")));
}

// AS 5488.2-2019 Table A.4, as the TfNSW Utility Schema's AssetTypeCode.
TEST(IfcUtilityClass, AssetTypeCodesAreThoseOfAs5488Part2TableA4)
{
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Telecommunications), "C");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Stormwater), "D");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Electricity), "E");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::FireService), "F");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Gas), "G");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::IntelligentTransport), "I");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Fuel), "P");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Sewer), "S");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Water), "W");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Unknown), "N");
    // The table has no code for these, and none is made up.
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::RecycledWater), "");
    EXPECT_EQ(ifc::assetTypeCode(sub::UtilityType::Other), "");
}

// ---- drawing entities ---------------------------------------------------------------

TEST(IfcEntityClass, TheTextIsTheLayerTheCodeAndThe12dNameInWordsOfCapitals)
{
    Entity kerb = point("Survey/Kerb_line", "KB01");
    EXPECT_EQ(ifc::classificationText(kerb), "SURVEY KERB LINE KB01");
    kerb.metadata["12d.name"] = std::string("lip of kerb");
    EXPECT_EQ(ifc::classificationText(kerb), "SURVEY KERB LINE KB01 LIP OF KERB");
}

TEST(IfcEntityClass, WordsMatchWholeAPrefixWithAStarAndAPhraseInSequence)
{
    EXPECT_TRUE(ifc::containsWord("SURVEY KERB KB01", "KERB"));
    EXPECT_TRUE(ifc::containsWord("SURVEY KERB KB01", "kerb"));
    EXPECT_FALSE(ifc::containsWord("SURVEY KERBSIDE", "KERB"));
    EXPECT_TRUE(ifc::containsWord("SURVEY KERBSIDE", "KERB*"));
    EXPECT_TRUE(ifc::containsWord("ELECTRICAL HV", "ELEC*"));
    EXPECT_FALSE(ifc::containsWord("SELECT", "ELEC*"));
    EXPECT_TRUE(ifc::containsWord("ROAD EDGE OF SEAL", "EDGE OF SEAL"));
    EXPECT_FALSE(ifc::containsWord("ROAD EDGE SEAL OF", "EDGE OF SEAL"));
    EXPECT_FALSE(ifc::containsWord("ROAD", ""));
    // STORMWATER is one word: it is not WATER.
    EXPECT_FALSE(ifc::containsWord("STORMWATER PITS", "WATER"));
}

TEST(IfcEntityClass, AServiceSystemIsNamedByItsWordsTheMostSpecificFirst)
{
    EXPECT_EQ(ifc::serviceSystemFor("STORMWATER PITS"), "STORMWATER");
    EXPECT_EQ(ifc::serviceSystemFor("SW PIPES"), "STORMWATER");
    EXPECT_EQ(ifc::serviceSystemFor("SEWER MH"), "SEWAGE");
    EXPECT_EQ(ifc::serviceSystemFor("RECYCLED WATER"), "RECYCLEDWATER");
    EXPECT_EQ(ifc::serviceSystemFor("WATER MAINS"), "WATERSUPPLY");
    EXPECT_EQ(ifc::serviceSystemFor("NBN PIT"), "COMMUNICATION");
    EXPECT_EQ(ifc::serviceSystemFor("ELECTRICAL"), "ELECTRICAL");
    EXPECT_EQ(ifc::serviceSystemFor("SURVEY KERB"), "");
}

TEST(IfcEntityClass, TheDefaultRulesTypeServicesFeaturesAndRoadFurniture)
{
    const auto& rules = ifc::defaultClassificationRules();
    const auto of = [&](const Entity& entity) { return ifc::classifyEntity(entity, rules); };

    auto kerb = of(polyline("Survey/Kerb"));
    EXPECT_EQ(kerb.ifcClass, cls("IfcKerb", "NOTDEFINED"));
    EXPECT_EQ(kerb.system, "");
    EXPECT_EQ(kerb.rule, "kerb");

    EXPECT_EQ(of(polyline("Survey/Fence")).ifcClass, cls("IfcRailing", "FENCE"));
    EXPECT_EQ(of(polyline("Retaining Wall")).ifcClass, cls("IfcWall", "RETAININGWALL"));
    EXPECT_EQ(of(polyline("Contours/Major")).ifcClass, cls("IfcAnnotation", "CONTOURLINE"));

    // A point on a pits layer is a pit in the layer's system; a line on the
    // same layer is the service, not a pit.
    const auto pit = of(point("Stormwater/Pits"));
    EXPECT_EQ(pit.ifcClass, cls("IfcDistributionChamberElement", "INSPECTIONPIT"));
    EXPECT_EQ(pit.system, "STORMWATER");
    const auto pipe = of(polyline("Stormwater/Pits"));
    EXPECT_EQ(pipe.ifcClass, cls("IfcPipeSegment", "RIGIDSEGMENT"));
    EXPECT_EQ(pipe.system, "STORMWATER");

    Entity manhole;
    manhole.geometry = Circle2{Point2{0.0, 0.0}, 0.6};
    manhole.layer = "SEWER MH";
    EXPECT_EQ(of(manhole).ifcClass, cls("IfcDistributionChamberElement", "MANHOLE"));
    EXPECT_EQ(of(manhole).system, "SEWAGE");

    // A rule may say its system: a hydrant is fire protection wherever it is.
    const auto hydrant = of(point("Survey/Points", "FH"));
    EXPECT_EQ(hydrant.ifcClass, cls("IfcFireSuppressionTerminal", "FIREHYDRANT"));
    EXPECT_EQ(hydrant.system, "FIREPROTECTION");

    EXPECT_EQ(of(polyline("Comms/Conduits")).ifcClass,
              cls("IfcCableCarrierSegment", "CONDUITSEGMENT"));
    EXPECT_EQ(of(polyline("Comms/Conduits")).system, "COMMUNICATION");
    EXPECT_EQ(of(point("Geotech", "BH")).ifcClass, cls("IfcBorehole"));
}

// What no rule names is an IfcAnnotation of the kind it is - never a proxy.
TEST(IfcEntityClass, WhatNoRuleNamesIsTheAnnotationOfItsKind)
{
    const auto& rules = ifc::defaultClassificationRules();
    const auto of = [&](const Entity& entity) {
        return ifc::classifyEntity(entity, rules).ifcClass;
    };

    EXPECT_EQ(of(point("Survey/Points")), cls("IfcAnnotation", "SURVEY"));
    EXPECT_EQ(of(polyline("Linework")), cls("IfcAnnotation", "NOTDEFINED"));
    Entity surveyed = polyline("Linework");
    katana::entity::setHeights(surveyed.properties, {20.0, 20.1, std::nullopt});
    EXPECT_EQ(of(surveyed), cls("IfcAnnotation", "SURVEY"));

    Entity text;
    text.geometry = katana::entity::TextGeometry{Point2{0.0, 0.0}, "LOT 42"};
    EXPECT_EQ(of(text), cls("IfcAnnotation", "TEXT"));

    // Across every kind and a spread of layer names, with the default rules.
    for (const char* layer :
         {"0", "Survey/Kerb", "SEWER", "Stormwater/Pits", "Trees", "Proxy", "Building", "Misc"}) {
        for (Entity entity : {point(layer), polyline(layer), text}) {
            entity.layer = layer;
            EXPECT_NE(of(entity).entity, "IfcBuildingElementProxy") << layer;
        }
    }
    for (const auto& rule : rules) {
        EXPECT_NE(rule.target.entity, "IfcBuildingElementProxy") << rule.name;
    }
}

// A project whose layers say otherwise passes its own rules, which replace
// the defaults.
TEST(IfcEntityClass, AProjectsOwnRulesReplaceTheDefaults)
{
    const std::vector<ifc::ClassificationRule> rules{
        {"light pole",
         {"POLE*", "LP"},
         {katana::entity::EntityType::Point},
         cls("IfcColumn", "COLUMN"),
         {}},
        {"drainage line", {"DRN"}, {}, cls("IfcPipeSegment", "RIGIDSEGMENT"), "STORMWATER"},
    };
    const auto pole = ifc::classifyEntity(point("Lighting", "LP"), rules);
    EXPECT_EQ(pole.ifcClass, cls("IfcColumn", "COLUMN"));
    EXPECT_EQ(pole.rule, "light pole");
    // The kind limits a rule: a line on a pole layer is not a pole.
    EXPECT_EQ(ifc::classifyEntity(polyline("Poles"), rules).ifcClass,
              cls("IfcAnnotation", "NOTDEFINED"));
    const auto drain = ifc::classifyEntity(polyline("DRN"), rules);
    EXPECT_EQ(drain.system, "STORMWATER");
    // The defaults are not consulted.
    EXPECT_EQ(ifc::classifyEntity(polyline("Survey/Kerb"), rules).ifcClass,
              cls("IfcAnnotation", "NOTDEFINED"));
}
