#pragma once

// CUSTOMISE with no files: what customisation is loaded, which files it came
// from, which files the project was drawn with that are not loaded, and what
// it covers in THIS drawing. One result and one formatter, as code_table.hpp
// has them, so the window's command line and katana_cli say the same words;
// the window refused a bare CUSTOMISE with its usage until 2026-09-26, and
// logged the counts only at startup and after a load.
//
// The sources arrive by name and what each brought (customisation_record.hpp).
// Both lists are the Document's (customisationState): the sources loaded this
// session in load order, and the names the last open found the project
// recorded but not loaded, less those loaded since. They are still parameters
// here so that the report can be tested without loading anything.

#include <cstddef>
#include <string>
#include <vector>

#include "katana/cad/customisation_record.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_coding.hpp"

namespace katana::cad {

struct CustomisationSummary {
    std::size_t definitions = 0; // in the style library
    std::size_t groups = 0;      // entity::styleGroups
    // Library definitions the pickers offer as each (decision D3,
    // symbolChoices and linetypeChoices); one definition can be both.
    std::size_t symbols = 0;
    std::size_t linestyles = 0;
    std::size_t rules = 0; // survey code rules
    std::size_t codes = 0; // distinct keys among them
    std::vector<CustomisationSource> loaded{};
    std::vector<std::string> notLoaded{};
    CustomisationCoverage coverage{};
};

[[nodiscard]] CustomisationSummary
customisationSummary(const Document& document, const std::vector<CustomisationSource>& loaded,
                     const std::vector<std::string>& missingAtOpen);

// Plain text, lines ending in '\n'. With nothing loaded it says so and how to
// load, and nothing more but the project's missing files: there is no coverage
// to report of nothing.
[[nodiscard]] std::string formatCustomisationSummary(const CustomisationSummary& summary);

// The two above in one call: what a bare CUSTOMISE prints.
[[nodiscard]] std::string customisationReport(const Document& document,
                                              const std::vector<CustomisationSource>& loaded,
                                              const std::vector<std::string>& missingAtOpen);

} // namespace katana::cad
