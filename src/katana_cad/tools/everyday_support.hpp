#pragma once

// What the everyday families share - Inquiry (inquiry.cpp), Divide and
// Measure (draw_divide.cpp), Lengthen and Reverse (modify_length.cpp), Match
// Properties (modify_properties.cpp), Select Similar and Quick Select
// (select_tools.cpp): numbers and directions as the survey reports print them,
// the survey point under a pick, a selection step, and an object measured
// along its length. In a family namespace, as modify_edit's helpers are,
// because other families are written at the same time and may well have
// helpers of the same names.

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/interactive_tool.hpp"
#include "katana/cad/survey_tools.hpp"

namespace katana::cad::tools::everyday {

using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Geometry;
using katana::geometry::Point2;
using katana::geometry::Vec2;

// ---- words and numbers --------------------------------------------------------------
//
// The survey reports' conventions (survey_tools.hpp): lengths and
// coordinates to 3 decimals, angles in degrees, minutes and seconds to
// kSurveySecondsDecimals, directions as azimuths clockwise from grid north
// and as quadrant bearings, "E ... N ..." easting first.

// `value` to `decimals` places, with no "-0.000".
[[nodiscard]] std::string fixed(double value, int decimals = 3);
// An angle in D°MM'SS.ss".
[[nodiscard]] std::string degrees(double radians);
// A quadrant bearing, N 36°52'11.63" E, of an azimuth.
[[nodiscard]] std::string bearing(double azimuth);
// The azimuth of `direction` in the drawing (x east, y north): clockwise from
// north, in [0, 2*pi). Zero for a zero vector, which callers refuse first.
[[nodiscard]] double azimuthOf(const Vec2& direction);
// "E 1000.000 N 2000.000".
[[nodiscard]] std::string coordinates(const Point2& point);
// The length unit reports print: "m" in a metre drawing, otherwise the
// project's own name for its unit.
[[nodiscard]] std::string lengthUnit(const Document& document);
// "12.000 m² (0.0012 ha)" in a metre drawing; "12.000 square feet" otherwise,
// where a hectare means nothing (formatAreaReport's rule).
[[nodiscard]] std::string areaText(const Document& document, double area);
// "1 entity", "3 entities": a count and its noun.
[[nodiscard]] std::string counted(std::size_t count, std::string_view one, std::string_view many);

// ---- the drawing -------------------------------------------------------------------

// How near a point entity a pick must be to BE that point in a report: a
// thousandth of a drawing unit, a millimetre in a drawing in metres. A snap
// lands on the point exactly, and coordinates typed from a report printed to
// three decimals are within half a thousandth on each axis (0.0007 in all).
// Not the view's pick aperture: at a zoom where the aperture is ten metres,
// a click that missed a point was reported as the point - its number, its
// coordinates, its height - which a surveyor would take for a measurement
// to the mark (seen on a real survey import).
inline constexpr double kPointCoincidence = 0.001;

// Where a pick landed, for a survey report: the point entity there - its
// number and its height, so an inverse between two survey points reports
// their height difference - or the bare coordinates. A point entity counts
// within kPointCoincidence of `at`; the nearest wins, the lower id on a tie.
[[nodiscard]] SurveyPosition positionAt(const Document* document, const Point2& at);

// The selection an asking tool ends with (List, Reverse, Select Similar, the
// targets of Match Properties): single picks the view handed to entity(),
// plus the document's selection when Enter is pressed - the view selects as
// the Select tool does while a tool asks for a selection - or everything
// selectable after "All". Ascending, without duplicates or deleted ids.
class SelectionStep {
  public:
    explicit SelectionStep(const Document* document) : document_(document) {}

    // A pick: Rejected for an id not in the drawing or picked already.
    [[nodiscard]] ToolStep pick(EntityId id);
    // "All": every entity the document lets be selected.
    [[nodiscard]] ToolStep all();
    // Takes back the last pick or All; false when there is none.
    bool undo();
    // What Enter takes.
    [[nodiscard]] std::vector<EntityId> chosen() const;

  private:
    const Document* document_ = nullptr;
    std::vector<EntityId> picked_;
    std::vector<std::vector<EntityId>> history_;
};

// ---- along an object ----------------------------------------------------------------
//
// A line, an arc, a circle or a polyline measured from its start - a circle
// from its east point, counter-clockwise, as it is drawn. Nullopt for a kind
// with no length (a point, a text, a dimension).
class Path {
  public:
    [[nodiscard]] static std::optional<Path> of(const Geometry& geometry);

    [[nodiscard]] double length() const { return length_; }
    [[nodiscard]] bool closed() const { return closed_; }
    // The point `station` along, clamped to [0, length].
    [[nodiscard]] Point2 pointAt(double station) const;

  private:
    Geometry geometry_;
    double length_ = 0.0;
    bool closed_ = false;
    std::vector<double> stations_; // polyline vertex stations, closing vertex included
};

} // namespace katana::cad::tools::everyday
