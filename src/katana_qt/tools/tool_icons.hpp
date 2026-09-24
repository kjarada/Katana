#pragma once

// Icons for the catalogue's interactive tools, by tool id
// (include/katana/cad/interactive_tool.hpp). A tool's icon lives with its
// family - tools/icons_<family>.cpp paints the family's tools - so a tool is
// added, drawn and tested in its family's files without editing a shared table.
//
// The painting conventions are the application's own (icons.cpp): a 24-unit
// grid, a 1.7-unit stroke, a neutral tone for the object and the accent for
// what the command does to it. ToolInk carries them.

#include <string>
#include <string_view>
#include <vector>

#include <QColor>
#include <QIcon>
#include <QPainterPath>
#include <QPointF>

class QPainter;

namespace katana::qt::tools {

// The design grid and stroke of icons.cpp, and the marks every icon is built
// from, so a node is the same node on every tool. This DUPLICATES icons.cpp's
// private Ink while that file is being changed by other work; merging the two
// into this header is a follow-up once icons.cpp is free (docs/cad.md).
inline constexpr double kGrid = 24.0;
inline constexpr double kStroke = 1.7;

class ToolInk {
  public:
    ToolInk(QPainter& painter, const QColor& neutral, const QColor& accent);

    void stroke(const QPainterPath& path, bool accent = false, double width = kStroke) const;
    void dashed(const QPainterPath& path, bool accent = false) const;
    void fill(const QPainterPath& path, bool accent = false, int alpha = 255) const;
    void line(double x1, double y1, double x2, double y2, bool accent = false,
              double width = kStroke) const;
    void dot(double x, double y, double radius, bool accent = false) const;
    // A vertex handle, as the viewport draws a grip: a small filled square.
    void node(double x, double y, bool accent = false) const;
    // Text on the grid (a dimension's figure, an "A" for Text), centred on
    // (x, y), `size` grid units tall.
    void label(double x, double y, double size, const QString& text, bool accent = false) const;

    [[nodiscard]] QPainter& painter() const { return painter_; }

  private:
    [[nodiscard]] const QColor& tone(bool accent) const { return accent ? accent_ : neutral_; }

    QPainter& painter_;
    QColor neutral_;
    QColor accent_;
};

[[nodiscard]] QPainterPath polyline(std::initializer_list<QPointF> points, bool closed = false);
[[nodiscard]] QPainterPath rectangle(double x, double y, double w, double h);
[[nodiscard]] QPainterPath circle(double x, double y, double radius);
// An arc of a circle, angles in degrees counter-clockwise from east as on
// paper (the painter's y points down; this flips it).
[[nodiscard]] QPainterPath arc(double cx, double cy, double radius, double startDegrees,
                               double sweepDegrees);

// The icon of tool `toolId`. A tool whose family paints nothing for it gets a
// plain glyph of its initial, so a missing icon is visible on the sheet rather
// than an empty button.
[[nodiscard]] QIcon toolIcon(std::string_view toolId);
// True when a family paints this tool.
[[nodiscard]] bool hasToolIcon(std::string_view toolId);

// ---- one painter per family (tools/icons_<family>.cpp) ------------------------------
//
// Each paints `toolId` on the 24-unit grid with `ink` and returns true, or
// returns false for an id that is not its family's. Listed explicitly, as the
// cad families are (src/katana_cad/tools/families.hpp).
bool paintDrawLineIcon(std::string_view toolId, const ToolInk& ink);
bool paintDrawCurveIcon(std::string_view toolId, const ToolInk& ink);
bool paintModifyTransformIcon(std::string_view toolId, const ToolInk& ink);
bool paintModifyEditIcon(std::string_view toolId, const ToolInk& ink);
bool paintAnnotateIcon(std::string_view toolId, const ToolInk& ink);
bool paintInquiryIcon(std::string_view toolId, const ToolInk& ink);
// icons_everyday.cpp: Divide and Measure, Lengthen and Reverse, Match
// Properties, Select Similar and Quick Select.
bool paintDrawDivideIcon(std::string_view toolId, const ToolInk& ink);
bool paintModifyLengthIcon(std::string_view toolId, const ToolInk& ink);
bool paintPropertyIcon(std::string_view toolId, const ToolInk& ink);
bool paintSelectIcon(std::string_view toolId, const ToolInk& ink);

} // namespace katana::qt::tools
