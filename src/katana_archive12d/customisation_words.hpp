#pragma once

// The words the two older customisation files use for entity's enumerations:
// the section elements of a survey code file (.mapfile) and the three kinds
// of definition in a style library (.4d). Internal to the module.
//
// They are those FILES' words, so they are kept here, with the reader and the
// writer of each and in both directions. entity's own words for the same
// values are Katana's - entity::toString(SurveySection) is "feature", which is
// what a person reads in the window and writes in a Katana customisation -
// and the two may differ exactly because neither is derived from the other.

#include <array>
#include <optional>
#include <string_view>

#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::archive12d::detail {

struct SectionElement {
    katana::entity::SurveySection section;
    std::string_view element;
};

inline constexpr std::array<SectionElement, 9> kSectionElements{{
    {katana::entity::SurveySection::Map, "map_data"},
    {katana::entity::SurveySection::VertexSymbol, "vertex_symbol_data"},
    {katana::entity::SurveySection::VertexTextStyle, "vertex_textstyle_data"},
    {katana::entity::SurveySection::Pipe, "pipe_data"},
    {katana::entity::SurveySection::VertexPipe, "vertex_pipe_data"},
    {katana::entity::SurveySection::SegmentPipe, "segment_pipe_data"},
    {katana::entity::SurveySection::StringAttribute, "string_attribute_data"},
    {katana::entity::SurveySection::VertexAttribute, "vertex_attribute_data"},
    {katana::entity::SurveySection::Tinable, "tinable_data"},
}};

// The element a section is written as.
[[nodiscard]] constexpr std::string_view sectionElement(katana::entity::SurveySection section)
{
    for (const SectionElement& entry : kSectionElements) {
        if (entry.section == section) {
            return entry.element;
        }
    }
    return kSectionElements.front().element;
}

// The section an element holds, or nullopt for one this does not know - so
// that a file with a tenth section is told its rules were not read.
[[nodiscard]] constexpr std::optional<katana::entity::SurveySection>
sectionOfElement(std::string_view element)
{
    // A mapfile writes the symbol section twice, once in a form marked v9.
    // Both say the same thing, so both are read as the same section.
    if (element == "vertex_symbol_data_v9") {
        return katana::entity::SurveySection::VertexSymbol;
    }
    for (const SectionElement& entry : kSectionElements) {
        if (entry.element == element) {
            return entry.section;
        }
    }
    return std::nullopt;
}

struct StyleKindWord {
    katana::entity::StyleUnits units;
    std::string_view word;
};

inline constexpr std::array<StyleKindWord, 3> kStyleKindWords{{
    {katana::entity::StyleUnits::World, "worldstyle"},
    {katana::entity::StyleUnits::Paper, "paperstyle"},
    {katana::entity::StyleUnits::TwoPoint, "twoptstyle"},
}};

// The word a definition of these units begins with.
[[nodiscard]] constexpr std::string_view styleKindWord(katana::entity::StyleUnits units)
{
    for (const StyleKindWord& entry : kStyleKindWords) {
        if (entry.units == units) {
            return entry.word;
        }
    }
    return kStyleKindWords.front().word;
}

// nullopt rather than a default for a word this does not know, so that a file
// using a fourth kind is reported instead of silently drawn as a worldstyle.
[[nodiscard]] constexpr std::optional<katana::entity::StyleUnits>
styleUnitsOfKind(std::string_view word)
{
    for (const StyleKindWord& entry : kStyleKindWords) {
        if (entry.word == word) {
            return entry.units;
        }
    }
    return std::nullopt;
}

} // namespace katana::archive12d::detail
