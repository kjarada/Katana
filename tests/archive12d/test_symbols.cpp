// PLAN.MD 20.2 slice 2: a 12d vertex symbol becomes the symbol on a Katana
// style, on a point; on a line it is kept and written back.
#include <gtest/gtest.h>

#include <algorithm>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/writer.hpp"
#include "katana/entity/model.hpp"

namespace a12 = katana::archive12d;
using katana::entity::Entity;

namespace {

// These fixtures are hand-written, and a hand-written 12da inherits the
// format's CURRENT BREAKLINE TYPE, whose default is `point` (commands,
// 1.4.4). A fixture that means a line has to say so, exactly as every string
// in a real archive does - 12d writes the flag on all 25,659 strings of a
// production file and leaves nothing to the default. Rather than repeat it in
// every fixture, the helper states it once at file level, which is the
// format's own way of saying it; a fixture that wants POINTS says
// `breakline point` inside the string and overrides this.
constexpr const char* kBreaklineLine = "breakline line\n";

a12::DomainImport import(const std::string& text)
{
    auto archive = a12::readArchive(kBreaklineLine + text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe());
    auto domain = a12::toDomain(archive.ok() ? *archive : a12::Archive{}, {});
    EXPECT_TRUE(domain.ok()) << (domain.ok() ? "" : domain.error().describe());
    return domain.ok() ? std::move(*domain) : a12::DomainImport{};
}

const katana::entity::Style* styleNamed(const a12::DomainImport& domain, const std::string& name)
{
    const auto found = std::find_if(domain.stylesNeeded.begin(), domain.stylesNeeded.end(),
                                    [&](const katana::entity::Style& s) { return s.name == name; });
    return found == domain.stylesNeeded.end() ? nullptr : &*found;
}

std::string joined(const std::vector<std::string>& warnings)
{
    std::string out;
    for (const auto& w : warnings) {
        out += "\n  " + w;
    }
    return out;
}

bool anyContains(const std::vector<std::string>& warnings, const std::string& needle)
{
    return std::any_of(warnings.begin(), warnings.end(),
                       [&](const std::string& w) { return w.find(needle) != std::string::npos; });
}

const std::string* meta(const Entity& entity, const std::string& key)
{
    const auto found = entity.metadata.find(key);
    return found == entity.metadata.end() ? nullptr : std::get_if<std::string>(&found->second);
}

// Model built from an import, the way the application does it.
katana::entity::Model modelOf(const a12::DomainImport& domain)
{
    katana::entity::Model model;
    for (const auto& layer : domain.layersNeeded) {
        EXPECT_TRUE(model.layers.add(layer).ok());
    }
    for (const auto& style : domain.stylesNeeded) {
        EXPECT_TRUE(model.styles.add(style).ok());
    }
    for (const auto& entity : domain.entities) {
        EXPECT_TRUE(model.entities.add(entity).ok());
    }
    return model;
}

a12::Archive exportOf(const katana::entity::Model& model)
{
    auto exported = a12::fromDomain(model, {});
    EXPECT_TRUE(exported.ok());
    auto back = a12::readArchive(a12::writeArchive(exported.ok() ? exported->archive : a12::Archive{}));
    EXPECT_TRUE(back.ok()) << (back.ok() ? "" : back.error().describe());
    return back.ok() ? std::move(*back) : a12::Archive{};
}

// What 12d Model 15 writes on a surveyed manhole: the string's own linestyle
// is "0", the symbol's is the feature code's, and both share the colour.
constexpr const char* kManhole = R"(string super { name "PSMH" breakline point colour yellow style "0"
  data_3d { 100 200 5.5 }
  symbol_data { properties { style "SEWR Manhole Cover" colour yellow size 1 rotation 0 offset 0 raise 0 } } })";

} // namespace

TEST(SymbolImport, APointTakesItsSymbolsLinestyleAsItsStyleAndTheStyleCarriesTheShape)
{
    const auto domain = import(kManhole);
    ASSERT_EQ(domain.entities.size(), 1u);
    const Entity& point = domain.entities[0];
    EXPECT_EQ(point.style, "SEWR Manhole Cover");
    const auto* style = styleNamed(domain, "SEWR Manhole Cover");
    ASSERT_NE(style, nullptr);
    // The REAL 12d name, kept so a loaded symbol library can be matched
    // against it. What it DRAWS as without one is the shape its words
    // suggest, which is asserted where that guess lives.
    EXPECT_EQ(style->symbol, "SEWR Manhole Cover");
    EXPECT_EQ(katana::entity::builtInSymbolFor(style->symbol), "manhole")
        << "the name still says what it is";
    EXPECT_EQ(style->symbolSize, 1.0);
    EXPECT_EQ(style->description, "12d symbol");
    // The string's own linestyle "0" is not the point's style, but it is
    // kept so that export writes the string as it was.
    ASSERT_NE(meta(point, "12d.string_style"), nullptr);
    EXPECT_EQ(*meta(point, "12d.string_style"), "0");
    EXPECT_EQ(point.color, a12::standardColour("yellow"));
    // Defaults are not metadata: 12d writes them on every one of thousands.
    EXPECT_EQ(meta(point, "12d.symbol.rotation"), nullptr);
    EXPECT_EQ(meta(point, "12d.symbol.offset"), nullptr);
    EXPECT_EQ(meta(point, "12d.symbol.raise"), nullptr);
    EXPECT_EQ(meta(point, "12d.symbol.size"), nullptr) << "the size is on the style";
    EXPECT_EQ(meta(point, "12d.symbol.colour"), nullptr) << "the colour is on the entity";
    EXPECT_FALSE(anyContains(domain.warnings, "symbol")) << joined(domain.warnings);
}

TEST(SymbolImport, TheSymbolValueFormAndTheSymbolDataFormAreOneSymbol)
{
    const auto value = import(R"(string super { name p breakline point data_3d { 1 2 3 }
  symbol_value { style "ELEC Pole - Power" colour red size 2.5 rotation 0 offset 0 raise 0 } })");
    const auto data = import(R"(string super { name p breakline point data_3d { 1 2 3 }
  symbol_data { properties { style "ELEC Pole - Power" colour red size 2.5 rotation 0 offset 0 raise 0 } } })");
    ASSERT_EQ(value.entities.size(), 1u);
    ASSERT_EQ(data.entities.size(), 1u);
    EXPECT_EQ(value.entities[0].style, "ELEC Pole - Power");
    EXPECT_EQ(data.entities[0].style, "ELEC Pole - Power");
    EXPECT_EQ(value.stylesNeeded, data.stylesNeeded);
    EXPECT_EQ(styleNamed(value, "ELEC Pole - Power")->symbol, "ELEC Pole - Power");
    EXPECT_EQ(styleNamed(value, "ELEC Pole - Power")->symbolSize, 2.5);
}

TEST(SymbolImport, WhatIsNotOnTheStyleOrTheEntityStaysOnThePointAsMetadata)
{
    // A rotated, offset symbol in a colour Katana has no RGB for, at a size
    // that is not the size the style was given first.
    const auto domain = import(R"(string super { name a breakline point data_3d { 0 0 0 }
  symbol_value { style "CULT Sign Post" colour red size 1 } }
string super { name b breakline point data_3d { 5 5 0 }
  symbol_value { style "CULT Sign Post" colour "off yellow" size 3 rotation 45 offset 0.5 raise 0 } })");
    ASSERT_EQ(domain.entities.size(), 2u);
    const auto* style = styleNamed(domain, "CULT Sign Post");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->symbol, "CULT Sign Post") << "the name the fixture declares";
    EXPECT_EQ(katana::entity::builtInSymbolFor(style->symbol), "pole")
        << "a post is a pole, before a sign is a flag";
    EXPECT_EQ(style->symbolSize, 1.0) << "the first size seen";

    const Entity& b = domain.entities[1];
    EXPECT_EQ(b.style, "CULT Sign Post");
    ASSERT_NE(meta(b, "12d.symbol.size"), nullptr);
    EXPECT_EQ(*meta(b, "12d.symbol.size"), "3");
    EXPECT_EQ(*meta(b, "12d.symbol.rotation"), "45");
    EXPECT_EQ(*meta(b, "12d.symbol.offset"), "0.5");
    EXPECT_EQ(meta(b, "12d.symbol.raise"), nullptr);
    ASSERT_NE(meta(b, "12d.symbol.colour"), nullptr) << "no RGB for 'off yellow': the name is kept";
    EXPECT_EQ(*meta(b, "12d.symbol.colour"), "off yellow");

    // And every one of them goes back where it came from.
    const auto archive = exportOf(modelOf(domain));
    ASSERT_EQ(archive.elements.size(), 2u);
    const auto& outB = std::get<a12::VertexString>(archive.elements[1]);
    ASSERT_TRUE(outB.symbol.has_value());
    EXPECT_EQ(outB.symbol->text("style"), "CULT Sign Post");
    EXPECT_EQ(outB.symbol->text("colour"), "off yellow");
    EXPECT_EQ(outB.symbol->real("size"), 3.0);
    EXPECT_EQ(outB.symbol->real("rotation"), 45.0);
    EXPECT_EQ(outB.symbol->real("offset"), 0.5);
    EXPECT_EQ(outB.symbol->real("raise"), 0.0);
    const auto& outA = std::get<a12::VertexString>(archive.elements[0]);
    ASSERT_TRUE(outA.symbol.has_value());
    EXPECT_EQ(outA.symbol->real("size"), 1.0) << "the style's size";
    EXPECT_EQ(outA.symbol->text("colour"), "red");
}

TEST(SymbolImport, AStyleMetAsALineFirstGainsItsSymbolWhenAPointNamesIt)
{
    const auto domain = import(R"(string super { name l style "Tree" data_3d { 0 0 0  1 1 1 } }
string super { name p breakline point style "1" data_3d { 5 5 0 }
  symbol_value { style "Tree" colour green size 2 } })");
    const auto* style = styleNamed(domain, "Tree");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(katana::entity::builtInSymbolFor(style->symbol), "tree");
    EXPECT_EQ(style->symbolSize, 2.0);
    EXPECT_EQ(domain.stylesNeeded.size(), 2u) << "Tree and 1: one style per name";
}

TEST(SymbolImport, ALinesVertexSymbolsAreKeptAsOneListPerKeyAndWrittenBackWhole)
{
    const auto domain = import(R"(string super { name fence data_3d { 0 0 0  10 0 0  20 0 0 }
  symbol_data { properties { style "Post" colour red size 1 rotation 0 offset 0 raise 0 }
                properties { style "Post" colour red size 1 rotation 90 offset 0 raise 0 }
                properties { style "Gate Post" colour "dark green" size 1.5 } } })");
    ASSERT_EQ(domain.entities.size(), 1u);
    const Entity& fence = domain.entities[0];
    EXPECT_EQ(fence.style, "1") << "the string's own linestyle - 12d's default (manual 1.4.3) - "
                                   "and not the vertex symbol's";
    EXPECT_EQ(styleNamed(domain, "Post"), nullptr);
    ASSERT_NE(meta(fence, "12d.symbol.style"), nullptr);
    EXPECT_EQ(*meta(fence, "12d.symbol.style"), "Post Post \"Gate Post\"");
    EXPECT_EQ(*meta(fence, "12d.symbol.rotation"), "0 90 \"\"")
        << "a field the third block lacks is an empty item, so the columns stay aligned";
    EXPECT_EQ(*meta(fence, "12d.symbol.colour"), "red red \"dark green\"");
    EXPECT_TRUE(anyContains(domain.warnings, "1 strings carry vertex symbols"))
        << joined(domain.warnings);

    const auto archive = exportOf(modelOf(domain));
    ASSERT_EQ(archive.elements.size(), 1u);
    const auto& out = std::get<a12::VertexString>(archive.elements[0]);
    EXPECT_FALSE(out.symbol.has_value());
    ASSERT_EQ(out.symbols.size(), 3u);
    EXPECT_EQ(out.symbols[0].text("style"), "Post");
    EXPECT_EQ(out.symbols[1].real("rotation"), 90.0);
    EXPECT_EQ(out.symbols[2].text("style"), "Gate Post");
    EXPECT_EQ(out.symbols[2].text("colour"), "dark green");
    EXPECT_EQ(out.symbols[2].real("size"), 1.5);
    EXPECT_FALSE(out.symbols[2].contains("rotation")) << "an empty item is a field the block never had";
}

TEST(SymbolImport, ALineWithOneSymbolForEveryVertexIsWrittenBackInTheValueForm)
{
    const auto domain = import(R"(string super { name l data_3d { 0 0 0  10 0 0 }
  symbol_value { style "Peg" colour red size 0.5 } })");
    const auto archive = exportOf(modelOf(domain));
    const auto& out = std::get<a12::VertexString>(archive.elements.at(0));
    ASSERT_TRUE(out.symbol.has_value());
    EXPECT_TRUE(out.symbols.empty());
    EXPECT_EQ(out.symbol->text("style"), "Peg");
    EXPECT_EQ(out.symbol->real("size"), 0.5);
}

TEST(SymbolExport, APointDrawnInAStyleWithASymbolIsWrittenWithTheBlock12dDrawsItFrom)
{
    // A point made in Katana, never near 12d: its style says "manhole".
    katana::entity::Model model;
    katana::entity::Style style;
    style.name = "Manhole";
    style.symbol = "manhole";
    style.symbolSize = 1.2;
    ASSERT_TRUE(model.styles.add(style).ok());
    katana::entity::Style plain;
    plain.name = "Kerb";
    ASSERT_TRUE(model.styles.add(plain).ok());
    for (const char* name : {"Manhole", "Kerb"}) {
        Entity entity;
        entity.geometry = katana::entity::PointGeometry{katana::geometry::Point2(1.0, 2.0)};
        entity.layer = "0";
        entity.style = name;
        entity.color = katana::entity::Color{0, 0, 255, 255};
        ASSERT_TRUE(model.entities.add(entity).ok());
    }
    const auto archive = exportOf(model);
    ASSERT_EQ(archive.elements.size(), 2u);
    const auto& manhole = std::get<a12::VertexString>(archive.elements[0]);
    EXPECT_EQ(manhole.header.style, "Manhole");
    ASSERT_TRUE(manhole.symbol.has_value());
    EXPECT_EQ(manhole.symbol->text("style"), "Manhole");
    EXPECT_EQ(manhole.symbol->text("colour"), "blue") << "the point's colour is the symbol's";
    EXPECT_EQ(manhole.symbol->real("size"), 1.2);
    EXPECT_EQ(manhole.symbol->real("rotation"), 0.0);
    EXPECT_EQ(manhole.symbol->real("offset"), 0.0);
    EXPECT_EQ(manhole.symbol->real("raise"), 0.0);
    const auto& kerb = std::get<a12::VertexString>(archive.elements[1]);
    EXPECT_FALSE(kerb.symbol.has_value()) << "a style with no symbol writes none";
}

TEST(SymbolExport, TheManholeComesBackAsItWentAfterAFullRoundTrip)
{
    const auto first = import(kManhole);
    const auto archive = exportOf(modelOf(first));
    const auto& out = std::get<a12::VertexString>(archive.elements.at(0));
    EXPECT_EQ(out.header.style, "0") << "the string's own linestyle, not the symbol's";
    EXPECT_EQ(out.header.colour, "yellow");
    ASSERT_TRUE(out.symbol.has_value());
    EXPECT_EQ(out.symbol->text("style"), "SEWR Manhole Cover");
    EXPECT_EQ(out.symbol->text("colour"), "yellow");
    EXPECT_EQ(out.symbol->real("size"), 1.0);
    // And a second pass through Katana finds nothing changed.
    auto again = a12::toDomain(archive, {});
    ASSERT_TRUE(again.ok());
    EXPECT_EQ(again->entities.at(0).style, "SEWR Manhole Cover");
    EXPECT_EQ(again->entities.at(0).metadata, first.entities.at(0).metadata);
    EXPECT_EQ(again->stylesNeeded, first.stylesNeeded);
}

TEST(SymbolNames, TheShapeIsReadFromTheLinestyleNameMostSpecificWordFirst)
{
    // The forty commonest symbol linestyles in the real archives, and what
    // a survey plan draws for each.
    const std::pair<const char*, const char*> cases[] = {
        {"TOPO Natural Surface Point", "cross"},
        {"STNS Default MX Survey Mark", "target"},
        {"CULT Fence Post - Guide Post", "pole"},
        {"MISC Unidentified Pole", "pole"},
        {"ELEC Pole - Light", "pole"},
        {"ELEC Suspended Light", "star"},
        {"DRAIN Gully Pit Point", "manhole"},
        {"SEWR Manhole Cover", "manhole"},
        {"DRAIN Invert - 375 Dia", "cross"},
        {"COMM Telephone Single Concrete Pit", "manhole"},
        {"FOTO Vertical Control Point", "target"},
        {"WATR Stop Valve", "diamond"},
        {"WATR Hydrant", "diamond"},
        {"WATR Meter", "diamond"},
        {"TCS Traffic Signal Junction Box", "flag"},
        {"VEG Tree - Deciduous", "tree"},
        {"BDGE Pier-Column Point", "pole"},
        {"BUIL Floor Level", "cross"},
        {"MISC Bollard", "dot"},
        {"ELEC Cable Junction Box", "circle"},
        {"", "circle"},
    };
    for (const auto& [name, shape] : cases) {
        EXPECT_EQ(a12::symbolForLinestyle(name), shape) << name;
        EXPECT_TRUE(katana::entity::isBuiltInSymbolName(a12::symbolForLinestyle(name))) << name;
    }
}

TEST(SymbolImport, AStringColourAndASymbolColourThatDifferBothSurviveTheRoundTrip)
{
    // 12d writes the same colour name on the string and on its symbol in
    // every sample archive, but the format allows two, and a point then has
    // an appearance (the symbol's) and a string colour that is not it.
    // Neither may be lost: the entity is drawn in the symbol's colour, the
    // string's own name is written back on the string.
    const auto domain = import(R"(string super { name p breakline point colour red style "0"
  data_3d { 0 0 0 }
  symbol_value { style "WATR Stop Valve" colour blue size 1 } })");
    ASSERT_EQ(domain.entities.size(), 1u);
    const Entity& point = domain.entities[0];
    EXPECT_EQ(point.color, a12::standardColour("blue")) << "a point looks like its symbol";
    ASSERT_NE(meta(point, "12d.colour"), nullptr);
    EXPECT_EQ(*meta(point, "12d.colour"), "red") << "the string's own colour is still the string's";
    ASSERT_NE(meta(point, "12d.symbol.colour"), nullptr);
    EXPECT_EQ(*meta(point, "12d.symbol.colour"), "blue");

    const auto archive = exportOf(modelOf(domain));
    const auto& out = std::get<a12::VertexString>(archive.elements.at(0));
    EXPECT_EQ(out.header.colour, "red") << "the string goes back out the colour it came in";
    ASSERT_TRUE(out.symbol.has_value());
    EXPECT_EQ(out.symbol->text("colour"), "blue");
}
