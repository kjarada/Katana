#pragma once

// Text command line for a Document (PLAN.MD Phase 08 "command line").
//
// One line in, one command out: the interpreter only parses and then builds the
// same Command objects the GUI uses, so everything typed is validated, atomic
// and undoable. It has no GUI dependency; the desktop application and the
// headless `katana_cli` share it.
//
// Syntax
//   VERB arg arg ...                 verbs are case-insensitive, with aliases
//   points      12.5,40   absolute
//               @3,4      relative to the last point entered
//               @5<30     polar: distance < angle (degrees, counter-clockwise from +x)
//   angles      degrees
//   text        "quoted strings may contain spaces"
//   ids         entity ids as shown by LIST
// Numbers are parsed locale-independently ('.' is always the decimal point).
//
// HELP lists every command.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {

namespace annotation {
struct AnchoredPoint;
}

class CommandInterpreter {
  public:
    explicit CommandInterpreter(Document& document) : document_(document) {}

    // Runs one line. On success returns a short human-readable message.
    // Empty lines succeed with an empty message.
    [[nodiscard]] katana::core::Result<std::string> run(std::string_view line);

    // Every non-empty line passed to run(), oldest first.
    [[nodiscard]] const std::vector<std::string>& history() const { return history_; }

    [[nodiscard]] static std::string helpText();

    // Forgets the "last point" that relative (@dx,dy) and polar (@d<a) points
    // resolve against. The interpreter cannot see a document being replaced
    // behind it, so a front end that swaps documents must say so - otherwise
    // "@10,10" after File > New silently measures from the discarded drawing.
    void resetPointState();

  private:
    using Tokens = std::vector<std::string>;
    using Reply = katana::core::Result<std::string>;

    [[nodiscard]] katana::core::Result<katana::geometry::Point2> parsePoint(const std::string& text);
    [[nodiscard]] Reply requireSelection() const;
    [[nodiscard]] Reply finish(katana::core::Status status, std::string message);

    [[nodiscard]] Reply draw(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply transform(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply edit(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply select(const Tokens& args);
    [[nodiscard]] Reply layer(const Tokens& args);
    [[nodiscard]] Reply linetype(const Tokens& args);
    [[nodiscard]] Reply dimensionStyle(const Tokens& args);
    [[nodiscard]] Reply hatchPattern(const Tokens& args);
    [[nodiscard]] Reply style(const Tokens& args);
    [[nodiscard]] Reply alignment(const Tokens& args);
    // CRS: the project's coordinate system - shown, set, cleared, found in the
    // common list, suggested for a place (project_crs.hpp).
    [[nodiscard]] Reply coordinateSystem(const Tokens& args);
    [[nodiscard]] Reply parcel(const Tokens& args);
    // INVERSE, FORWARD (RADIATE) and AREA: the survey tools of survey_tools.hpp,
    // printing the same report the Survey menu's dialogs print.
    [[nodiscard]] Reply survey(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply attributes(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply undoRedo(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply file(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply inspect(const std::string& verb, const Tokens& args) const;

    // The annotation verbs (annotation/annotation_verbs.cpp, docs/annotation.md):
    // ANNOSCALE, TEXTSTYLE, TEXT and MTEXT with options, TEXTEDIT, LABELSTYLE,
    // LABEL, AUTOLABEL, the DIM kinds, LEADER and BALLOON. Options are
    // key=value; replies are key=value records, one per line.
    [[nodiscard]] static bool isAnnotationVerb(const std::string& verb, const Tokens& args);
    [[nodiscard]] static std::string annotationHelpText();
    [[nodiscard]] Reply annotation(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply annotationScale(const Tokens& args);
    [[nodiscard]] Reply textStyle(const Tokens& args);
    [[nodiscard]] Reply styledText(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply textEdit(const Tokens& args);
    [[nodiscard]] Reply labelStyle(const Tokens& args);
    [[nodiscard]] Reply label(const Tokens& args);
    [[nodiscard]] Reply autoLabel(const Tokens& args);
    [[nodiscard]] Reply dimension(const Tokens& args);
    [[nodiscard]] Reply leader(const std::string& verb, const Tokens& args);
    // A point that may name another entity's point - "#12", "#12.end",
    // "#12.v3", "#12.s2", "#12.mid", "#12.centre" - or any point parsePoint
    // takes. A named point carries its reference, so what is made from it
    // follows the entity (annotation/associative.hpp).
    [[nodiscard]] katana::core::Result<annotation::AnchoredPoint>
    parseAnchoredPoint(const std::string& text);

    Document& document_;
    std::vector<std::string> history_;
    std::optional<katana::geometry::Point2> lastPoint_;
};

} // namespace katana::cad
