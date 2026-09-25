// A schedule written from the model (writeUtilityCsv, utility_csv.hpp) reads
// back as the lines it was written from - field for field, doubles to the
// bit - so that a drawing edited in CAD becomes a deliverable again; and in a
// client's words when a delivery schema is given (utilityCsvDialect).

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/survey/subsurface/delivery_schema.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"

using namespace katana::survey::subsurface;
using katana::core::ErrorCode;

namespace {

std::vector<UtilityLine> readBack(const std::vector<UtilityLine>& lines,
                                  const UtilityCsvDialect& dialect = {})
{
    const auto text = writeUtilityCsv(lines, dialect);
    EXPECT_TRUE(text.ok()) << text.error().describe();
    if (!text.ok()) {
        return {};
    }
    const auto back = parseUtilityCsv(*text);
    EXPECT_TRUE(back.ok()) << back.error().describe() << "\n" << *text;
    return back.ok() ? *back : std::vector<UtilityLine>{};
}

UtilityVertex vertex(std::string id, double easting, double northing,
                     LocationMethod method = LocationMethod::ElectromagneticLocation)
{
    UtilityVertex out;
    out.id = std::move(id);
    out.position = {northing, easting};
    out.evidence.method = method;
    return out;
}

// Every field the format has, set.
UtilityLine everything()
{
    UtilityLine line;
    line.id = "W-0001";
    line.attributes.type = UtilityType::RecycledWater;
    line.attributes.status = UtilityStatus::Abandoned;
    line.attributes.owner = "Water Co, North";
    line.attributes.material = "\"DICL\" lined";
    line.attributes.diameter = 0.375;
    line.attributes.diameterIsInside = true;
    line.attributes.configuration = "4 x 100";
    line.attributes.description = "#1 trunk main ";
    line.attributes.fields = {{"Capacity", "210kPa"}, {"Clash", "Soft"}};
    UtilityVertex a = vertex("P1", 334000.125, 6250000.1);
    a.level = 19.2;
    a.levelReference = LevelReference::Centre;
    a.surfaceLevel = 20.25;
    a.evidence.horizontalUncertainty = 0.1;
    a.evidence.verticalUncertainty = 0.35;
    a.claimed = QualityLevel::B;
    a.fields = {{"DateInfoObtained", "2026/09/20"}};
    UtilityVertex b = vertex(" P2", 334008.0, 6250000.15, LocationMethod::NonDestructiveExcavation);
    b.depth = 0.95;
    b.surfaceLevel = 20.3;
    b.verifies = "P1";
    b.evidence.hasLevel = true;
    UtilityVertex c = vertex("P3", 334020.0, 6250000.2, LocationMethod::Unknown);
    c.levelReference = LevelReference::Unknown;
    a.evidence.hasLevel = true;
    line.vertices = {a, b, c};
    line.pathEvidence = {PathEvidence::Exposed, PathEvidence::Assumed};
    return line;
}

} // namespace

TEST(SubsurfaceScheduleWriter, EveryFieldReadsBackExactly)
{
    const std::vector<UtilityLine> lines{everything()};
    EXPECT_EQ(readBack(lines), lines);
}

TEST(SubsurfaceScheduleWriter, EveryWordValueReadsBackAsItself)
{
    std::vector<UtilityLine> lines;
    for (const UtilityType type :
         {UtilityType::Unknown, UtilityType::Electricity, UtilityType::Telecommunications,
          UtilityType::Gas, UtilityType::Water, UtilityType::RecycledWater,
          UtilityType::FireService, UtilityType::Sewer, UtilityType::Stormwater,
          UtilityType::Fuel, UtilityType::IntelligentTransport, UtilityType::Other}) {
        UtilityLine line;
        line.id = std::string("T") + toString(type);
        line.attributes.type = type;
        line.vertices = {vertex("a", 0, 0), vertex("b", 1, 0)};
        lines.push_back(line);
    }
    for (const UtilityStatus status : {UtilityStatus::Unknown, UtilityStatus::InService,
                                       UtilityStatus::Disused, UtilityStatus::Abandoned,
                                       UtilityStatus::Proposed}) {
        UtilityLine line;
        line.id = std::string("S") + toString(status);
        line.attributes.status = status;
        line.vertices = {vertex("a", 0, 0), vertex("b", 1, 0)};
        lines.push_back(line);
    }
    UtilityLine methods;
    methods.id = "M";
    for (const LocationMethod method :
         {LocationMethod::Unknown, LocationMethod::Records, LocationMethod::Anecdotal,
          LocationMethod::SurfaceFeature, LocationMethod::ElectromagneticLocation,
          LocationMethod::GroundPenetratingRadar, LocationMethod::OtherGeophysical,
          LocationMethod::NonDestructiveExcavation, LocationMethod::OpenExcavation}) {
        methods.vertices.push_back(vertex(std::string("m") + toString(method), 0, 0, method));
    }
    for (const LevelReference reference : {LevelReference::Top, LevelReference::Centre,
                                           LevelReference::Invert, LevelReference::Unknown}) {
        UtilityVertex v = vertex(std::string("r") + toString(reference), 0, 0);
        v.level = 10.0;
        v.levelReference = reference;
        v.evidence.hasLevel = true;
        methods.vertices.push_back(v);
    }
    for (const QualityLevel claim :
         {QualityLevel::A, QualityLevel::B, QualityLevel::C, QualityLevel::D}) {
        UtilityVertex v = vertex(std::string("q") + toString(claim), 0, 0);
        v.claimed = claim;
        methods.vertices.push_back(v);
    }
    methods.pathEvidence.assign(methods.vertices.size() - 1, PathEvidence::Detected);
    methods.pathEvidence[1] = PathEvidence::Exposed;
    methods.pathEvidence[2] = PathEvidence::Assumed;
    lines.push_back(methods);
    EXPECT_EQ(readBack(lines), lines);
}

TEST(SubsurfaceScheduleWriter, EveryWholeMillimetreDiameterReadsBackToTheBit)
{
    // A diameter read from a schedule is millimetres divided by 1000; the
    // writer must find the millimetres that divide back to it exactly.
    std::vector<UtilityLine> lines;
    for (int millimetres = 1; millimetres <= 3000; ++millimetres) {
        for (const bool inside : {false, true}) {
            UtilityLine line;
            line.id = std::to_string(millimetres) + (inside ? "i" : "o");
            line.attributes.diameter = millimetres / 1000.0;
            line.attributes.diameterIsInside = inside;
            line.vertices = {vertex("a", 0, 0), vertex("b", 1, 0)};
            lines.push_back(line);
        }
    }
    const std::vector<UtilityLine> back = readBack(lines);
    ASSERT_EQ(back.size(), lines.size());
    for (std::size_t i = 0; i < lines.size(); ++i) {
        EXPECT_EQ(back[i].attributes.diameter, lines[i].attributes.diameter) << lines[i].id;
        EXPECT_EQ(back[i].attributes.diameterIsInside, lines[i].attributes.diameterIsInside);
    }
}

TEST(SubsurfaceScheduleWriter, OnlyTheColumnsThatHoldSomethingAreWritten)
{
    UtilityLine line;
    line.id = "G1";
    line.vertices = {vertex("a", 1, 2, LocationMethod::Records), vertex("b", 3, 4)};
    const auto text = writeUtilityCsv({line});
    ASSERT_TRUE(text.ok());
    EXPECT_EQ(*text, "line,point,easting,northing,method\n"
                     "G1,a,1,2,records\n"
                     "G1,b,3,4,electromagnetic location\n");
    // Nothing at all is a header alone, which reads as no lines.
    const auto none = writeUtilityCsv({});
    ASSERT_TRUE(none.ok());
    EXPECT_EQ(*none, "line,point,easting,northing,method\n");
    EXPECT_TRUE(parseUtilityCsv(*none)->empty());
}

TEST(SubsurfaceScheduleWriter, WhatAScheduleCannotHoldIsRefusedByName)
{
    UtilityLine line = everything();
    line.vertices[1].verifies = "two\nlines";
    auto text = writeUtilityCsv({line});
    ASSERT_FALSE(text.ok());
    EXPECT_EQ(text.error().message,
              "line W-0001 point  P2: verifies holds a line break; a schedule cannot hold it");

    line = everything();
    line.attributes.fields.emplace("Colour", "blue");
    EXPECT_EQ(writeUtilityCsv({line}).error().code, ErrorCode::InvalidArgument);
    line = everything();
    line.vertices[0].fields.emplace("Capacity", "1");
    EXPECT_TRUE(writeUtilityCsv({line}).error().message.find("carried by the other") !=
                std::string::npos);
    line = everything();
    line.pathEvidence.pop_back();
    EXPECT_EQ(writeUtilityCsv({line}).error().code, ErrorCode::InvalidArgument);
    line = everything();
    line.vertices[2].id = "P1";
    EXPECT_TRUE(writeUtilityCsv({line}).error().message.find("a point id twice") !=
                std::string::npos);
    line = everything();
    line.vertices[1].depth = -0.5;
    EXPECT_EQ(writeUtilityCsv({line}).error().code, ErrorCode::InvalidArgument);
    line = everything();
    line.id.clear();
    EXPECT_EQ(writeUtilityCsv({line}).error().code, ErrorCode::InvalidArgument);
}

TEST(SubsurfaceScheduleWriter, ASchemasDialectNamesTheColumnsAndSpellsTheValuesItsWay)
{
    const auto schema = parseDeliverySchema(
        "kind,attribute,value,detail,label\n"
        "schema,Example,1,,\n"
        "field,AssetIdentifier,Alphanumerical,Yes,Asset Identifier\n"
        "field,AssetTypeCode,Domain List: Asset Type Code,Yes,Asset Type Code\n"
        "domain,AssetTypeCode,W,,\n"
        "domain,AssetTypeCode,E,,\n"
        "field,Situation,Domain List: Status,Yes,Status\n"
        "domain,Situation,Live,,\n"
        "domain,Situation,In Service,,\n"
        "field,DepthLocation,Domain List: Depth Location,Yes,Depth Location\n"
        "domain,DepthLocation,Top of Concrete Encasement,,\n"
        "domain,DepthLocation,Top of Pipe,,\n"
        "domain,DepthLocation,Top Row Invert,,\n");
    ASSERT_TRUE(schema.ok()) << schema.error().describe();
    const UtilityCsvDialect dialect = utilityCsvDialect(*schema);
    // By an alias of the column, and by a label when the attribute is none.
    EXPECT_EQ(dialect.headers.at("line"), "AssetIdentifier");
    EXPECT_EQ(dialect.headers.at("type"), "AssetTypeCode");
    EXPECT_EQ(dialect.headers.at("status"), "Status");
    EXPECT_EQ(dialect.headers.at("level_ref"), "DepthLocation");
    EXPECT_FALSE(dialect.headers.contains("method"));
    // The first listed spelling of each value.
    EXPECT_EQ(dialect.spellings.at("type").at("water"), "W");
    EXPECT_EQ(dialect.spellings.at("status").at("in service"), "Live");
    EXPECT_EQ(dialect.spellings.at("level_ref").at("top"), "Top of Concrete Encasement");
    EXPECT_EQ(dialect.spellings.at("level_ref").at("invert"), "Top Row Invert");

    UtilityLine line;
    line.id = "W-1";
    line.attributes.type = UtilityType::Water;
    line.attributes.status = UtilityStatus::InService;
    line.vertices = {vertex("a", 0, 0), vertex("b", 5, 0)};
    line.vertices[0].depth = 1.0;
    line.vertices[0].evidence.hasLevel = true;
    const auto text = writeUtilityCsv({line}, dialect);
    ASSERT_TRUE(text.ok());
    EXPECT_EQ(*text, "AssetIdentifier,point,easting,northing,method,DepthLocation,depth,"
                     "AssetTypeCode,Status\n"
                     "W-1,a,0,0,electromagnetic location,Top of Concrete Encasement,1,W,Live\n"
                     "W-1,b,5,0,electromagnetic location,,,W,Live\n");
    EXPECT_EQ(readBack({line}, dialect), std::vector<UtilityLine>{line});
}

TEST(SubsurfaceScheduleWriter, CellsTheReaderReadsAsNotRecordedAreWrittenAsTheScheduleWroteThem)
{
    // Each of these reads as "not recorded", or in part: a claim of Unknown,
    // a Depth Location with nothing measured on it, a type Not Specified, a
    // status Unknown, a size Not Applicable, a W x H size, a size beside a
    // diameter_mm.
    const auto lines = parseUtilityCsv(
        "line,point,easting,northing,method,level_ref,ql,type,status,size,diameter_mm\n"
        "A,a1,0,0,EML,Top of Pipe,Unknown,Not Specified,Unknown,Not Applicable,\n"
        "A,a2,5,0,EML,,Unknown,Not Specified,Unknown,Not Applicable,\n"
        "B,b1,0,1,EML,,QL-B,water,live,1200 x 900,\n"
        "B,b2,5,1,EML,,,water,live,1200 x 900,\n"
        "C,c1,0,2,EML,,,gas,,110,150\n"
        "C,c2,5,2,EML,,,gas,,110,150\n");
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    ASSERT_EQ(lines->size(), 3u);
    const UtilityLine& a = (*lines)[0];
    EXPECT_EQ(a.attributes.type, UtilityType::Unknown);
    EXPECT_EQ(a.attributes.status, UtilityStatus::Unknown);
    EXPECT_EQ(a.attributes.diameter, 0.0);
    EXPECT_FALSE(a.vertices[0].claimed.has_value());
    EXPECT_EQ(a.vertices[0].levelReference, a.vertices[1].levelReference);
    EXPECT_EQ((*lines)[1].attributes.diameter, 1.2);
    EXPECT_EQ((*lines)[2].attributes.diameter, 0.15);

    // Written back as they were written, and read back as they were read.
    const auto text = writeUtilityCsv(*lines);
    ASSERT_TRUE(text.ok()) << text.error().describe();
    EXPECT_EQ(*text,
              "line,point,easting,northing,method,level_ref,ql,type,diameter_mm,size,status\n"
              "A,a1,0,0,electromagnetic location,Top of Pipe,Unknown,Not Specified,,"
              "Not Applicable,Unknown\n"
              "A,a2,5,0,electromagnetic location,,Unknown,Not Specified,,Not Applicable,Unknown\n"
              "B,b1,0,1,electromagnetic location,,QL-B,water,,1200 x 900,in service\n"
              "B,b2,5,1,electromagnetic location,,,water,,1200 x 900,in service\n"
              "C,c1,0,2,electromagnetic location,,,gas,150,110,\n"
              "C,c2,5,2,electromagnetic location,,,gas,150,110,\n");
    EXPECT_EQ(parseUtilityCsv(*text).value(), *lines);

    // What a schedule words the writer's own way is not kept: nothing to say
    // again.
    const auto plain = parseUtilityCsv("line,point,easting,northing,method,size,level_ref,depth\n"
                                       "P,p1,0,0,EML,150,Top of Pipe,0.9\n"
                                       "P,p2,5,0,EML,150,,\n");
    ASSERT_TRUE(plain.ok());
    EXPECT_TRUE(plain->front().attributes.recorded.empty());
    EXPECT_TRUE(plain->front().vertices[0].recorded.empty());

    // An edit made since wins over what was kept: each kept cell is written
    // only while it still reads as the value held.
    std::vector<UtilityLine> edited = *lines;
    edited[0].attributes.status = UtilityStatus::InService;
    edited[0].vertices[0].claimed = QualityLevel::C;
    edited[0].vertices[0].level = 19.0;
    edited[1].attributes.diameter = 0.3;
    const auto rewritten = writeUtilityCsv(edited);
    ASSERT_TRUE(rewritten.ok());
    EXPECT_NE(rewritten->find(
                  "A,a1,0,0,electromagnetic location,19,top,QL-C,Not Specified,,Not Applicable,"
                  "in service\n"),
              std::string::npos)
        << *rewritten;
    EXPECT_NE(rewritten->find("B,b1,0,1,electromagnetic location,,,QL-B,water,,300,in service\n"),
              std::string::npos)
        << *rewritten;
}
