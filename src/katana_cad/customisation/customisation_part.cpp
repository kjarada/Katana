#include "katana/cad/customisation_part.hpp"

#include <algorithm>
#include <functional>
#include <set>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/entity/colour_names.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::cad {

using katana::entity::Customisation;

Customisation customisationPart(Customisation session,
                                const katana::entity::CustomisationWriteOptions& written)
{
    Customisation part = std::move(session);
    part.linework.reset();
    part.automation.reset();
    part.basedOn.reset();

    // What the write will hold, chosen as the writer chooses
    // (entity::customisationToJson): where each written definition came
    // from, and the colours the written definitions and rules name, by the
    // fold a colour name is found by.
    std::set<std::string, std::less<>> writtenFrom;
    std::set<std::string, std::less<>> named;
    const auto names = [&named](const std::string& colour) {
        if (!colour.empty()) {
            named.insert(katana::entity::foldColourName(colour));
        }
    };
    const std::set<std::string_view> only(written.only.begin(), written.only.end());
    part.library.forEach([&](const katana::entity::LineStyle& definition) {
        const bool kind = definition.symbol ? written.symbols : written.linestyles;
        if (!kind || (!only.empty() && !only.contains(definition.name))) {
            return;
        }
        writtenFrom.insert(definition.source);
        for (const katana::entity::Stroke& stroke : definition.strokes) {
            if (stroke.op == katana::entity::StrokeOp::Pen) {
                names(stroke.pen);
            }
        }
    });
    const bool rulesWritten = written.codes && !part.map.empty();
    if (rulesWritten) {
        // The three places a rule names a colour (the lint's own three,
        // cad::lintSurveyRule).
        for (const katana::entity::SurveyRule& rule : part.map.rules()) {
            names(rule.colour);
            if (rule.symbol) {
                names(rule.symbol->colour);
            }
            if (rule.textStyle) {
                names(rule.textStyle->colour);
            }
        }
    }
    katana::entity::ColourTable carried;
    for (const katana::entity::ColourTable::Entry& colour : part.colours.entries()) {
        if (named.contains(katana::entity::foldColourName(colour.name))) {
            // Cannot be refused: the entry comes out of a table that took it.
            (void)carried.add(colour.name, colour.colour);
        }
    }
    part.colours = std::move(carried);

    std::vector<katana::entity::CustomisationSourceNote> held;
    held.reserve(part.sources.size());
    for (katana::entity::CustomisationSourceNote& source : part.sources) {
        const bool broughtNeither = !source.definitions && !source.rules;
        source.definitions = writtenFrom.contains(source.name);
        source.rules = source.rules && rulesWritten;
        if (source.definitions || source.rules || (broughtNeither && !part.colours.empty())) {
            held.push_back(std::move(source));
            continue;
        }
        for (std::string& line : source.notice) {
            if (std::find(part.notice.begin(), part.notice.end(), line) == part.notice.end()) {
                part.notice.push_back(std::move(line));
            }
        }
    }
    part.sources = std::move(held);
    return part;
}

DefinitionsOfFile definitionsOfFile(Customisation file)
{
    DefinitionsOfFile result;
    result.rulesLeft = file.map.size();
    result.lineworkLeft = file.linework.has_value();
    result.automationLeft = file.automation.has_value();
    file.map = {};
    file.linework.reset();
    file.automation.reset();
    const bool withColours = !file.colours.empty();
    std::vector<katana::entity::CustomisationSourceNote> standing;
    standing.reserve(file.sources.size());
    for (katana::entity::CustomisationSourceNote& source : file.sources) {
        const bool neither = !source.definitions && !source.rules;
        if (!source.definitions && !(neither && withColours)) {
            continue;
        }
        source.rules = false;
        standing.push_back(std::move(source));
    }
    file.sources = std::move(standing);
    result.taken = std::move(file);
    return result;
}

} // namespace katana::cad
