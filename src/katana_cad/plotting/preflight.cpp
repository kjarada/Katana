#include "katana/cad/plotting/preflight.hpp"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <format>
#include <map>
#include <system_error>
#include <utility>

#include <nlohmann/json.hpp>

#include "katana/cad/dimension_draw.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/hatching.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/key_plan.hpp"
#include "katana/cad/plotting/legend.hpp"
#include "katana/cad/plotting/page_setup.hpp"
#include "katana/cad/plotting/plan_grid.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/tables.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/spatial_query.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/alignment.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Vec2;
using Json = nlohmann::json;

namespace {

constexpr std::array<CheckDescription, 33> kChecks{{
    // The set's own: what every sheet shares.
    {"field.empty", Severity::Warning,
     "a value the frame prints that resolves to nothing: the organisation, a sign-off name, the "
     "height datum, the coordinate system... listed once for the set"},
    {"logo.missing", Severity::Info, "no logo in the title block's logo slot"},
    {"logo.unreadable", Severity::Warning, "a logo named that is not in the project or cannot be read"},
    {"pagesetup.invalid", Severity::Error,
     "a page setup the plot refuses: a resolution or line weight scale out of range, or a file-name "
     "pattern that cannot be expanded"},
    // Each sheet's own.
    {"sheet.duplicate-id", Severity::Error, "a sheet with no id, or the id of an earlier sheet"},
    {"sheet.duplicate-name", Severity::Warning, "a sheet with the name of an earlier sheet"},
    {"frame.unknown", Severity::Error, "a frame this build does not have: no title block prints"},
    {"portrait.no-frame", Severity::Warning,
     "a portrait sheet asking for a frame, which is laid out for landscape paper"},
    {"sheet.empty", Severity::Warning, "a sheet with no views"},
    // Each viewport's, in this order.
    {"viewport.duplicate-id", Severity::Error, "a viewport with no id, or the id of an earlier one"},
    {"viewport.unplaced", Severity::Warning, "a viewport not placed on the paper, which does not print"},
    {"viewport.outside", Severity::Error,
     "a viewport off the paper or under the title block (an error), or past the drawing area into "
     "the margin (a warning)"},
    {"viewport.overlap", Severity::Warning,
     "a viewport covering part of an earlier one (a panel set inside a view is an inset: info)"},
    {"viewport.too-small", Severity::Info, "a viewport smaller than the smallest its kind reads at"},
    {"scale.invalid", Severity::Error, "a scale that is not a positive number"},
    {"scale.non-standard", Severity::Info, "a fixed scale that is not on the sheet scale ladder"},
    {"plan.alignment-missing", Severity::Warning, "a plan following an alignment the drawing does not have"},
    {"plan.empty", Severity::Warning,
     "a plan whose window - at its scale, about its centre, turned by its rotation - holds nothing "
     "it draws"},
    {"text.too-small", Severity::Warning,
     "drawing text in a plan that prints smaller than the minimum at the plan's scale"},
    {"grid.invalid", Severity::Error,
     "a coordinate grid that cannot be drawn: its lines would print closer than the least spacing"},
    {"grid.empty", Severity::Info, "a coordinate grid whose interval is wider than the plan's window"},
    {"section.alignment-missing", Severity::Error,
     "a section with no alignment, or naming one the drawing does not have"},
    {"section.alignment-invalid", Severity::Error, "a section of an alignment that cannot be solved"},
    {"section.no-stations", Severity::Error, "cross sections with no chainage to cut at"},
    {"section.station-outside", Severity::Error,
     "a cross section's chainage off the alignment (an error), or a long section's range running "
     "past its end (a warning)"},
    {"section.no-surface", Severity::Error, "a section with no surface to cut (nor a design profile)"},
    {"image.missing", Severity::Error, "an image panel naming no file, or one the project does not have"},
    {"notes.empty", Severity::Info, "a notes panel with no text"},
    {"legend.empty", Severity::Info, "a legend whose plans show nothing, so it lists nothing"},
    {"legend.overflow", Severity::Warning,
     "a legend with more entries than fit, which prints \"+N more\" for the rest"},
    {"revisions.empty", Severity::Info, "a revision table on a set with no revisions"},
    {"table.overflow", Severity::Warning,
     "a drawing register or revision table whose rows do not all fit"},
    {"matchline.dangling", Severity::Warning,
     "a match line or key-plan outline leading to a sheet that no longer exists"},
}};

// A viewport as a person finds it: its title as printed, and its id.
std::string labelOf(const Viewport& viewport)
{
    std::string title = viewport.title.empty() ? automaticTitle(viewport) : viewport.title;
    if (title.empty()) {
        title = "IMAGE";
    }
    return title + " (" + viewport.id + ")";
}

std::string mm(double value)
{
    return std::format("{:.1f} mm", value);
}

std::string chainage(double value)
{
    return std::format("CH {:.3f}", value);
}

std::string scaleName(double denominator)
{
    return std::format("1:{:.0f}", denominator);
}

std::string trimmedUpper(std::string_view text)
{
    const auto first = text.find_first_not_of(" \t\r\n");
    if (first == std::string_view::npos) {
        return {};
    }
    const auto last = text.find_last_not_of(" \t\r\n");
    return katana::core::uppered(text.substr(first, last - first + 1));
}

bool isBlank(std::string_view text)
{
    return text.find_first_not_of(" \t\r\n") == std::string_view::npos;
}

bool isScaled(ViewportKind kind)
{
    return kind == ViewportKind::Plan || kind == ViewportKind::KeyPlan ||
           kind == ViewportKind::LongSection || kind == ViewportKind::CrossSections;
}

// A panel set inside a view on purpose - a legend or key plan in a plan's
// corner - is an inset, not a mistake.
bool isPanel(ViewportKind kind)
{
    return kind == ViewportKind::Legend || kind == ViewportKind::Notes ||
           kind == ViewportKind::Image || kind == ViewportKind::KeyPlan ||
           kind == ViewportKind::SheetIndex || kind == ViewportKind::Revisions;
}

// ---- the title block's blanks -----------------------------------------------------------

// What each field the frame prints is called on the Title Block, and how much
// a blank matters. Nothing: a blank is expected (the optional project lines,
// the client, the notes). The order is the order findings come in.
struct FieldRule {
    std::string_view field;
    std::string_view label;
    std::optional<Severity> severity;
    std::string_view fix;
};

constexpr std::string_view kProjectTab = "Fill it in on Title Block > Project";
constexpr std::string_view kSignOffTab = "Fill it in on Title Block > Sign-offs";

constexpr std::array<FieldRule, 27> kFieldRules{{
    {"organisation", "Organisation", Severity::Warning, kProjectTab},
    {"project_line_1", "Project line 1", Severity::Warning,
     "Fill it in on Title Block > Project, or give the project a name"},
    {"project_line_2", "Project line 2", Severity::Info,
     "Fill it in on Title Block > Project, or give the project a description"},
    {"project_line_3", "Project line 3", std::nullopt, kProjectTab},
    {"project_line_4", "Project line 4", std::nullopt, kProjectTab},
    {"client", "Client", std::nullopt, kProjectTab},
    {"set_number", "Drawing set number", Severity::Info, kProjectTab},
    {"coordinate_system", "Coordinate system", Severity::Warning,
     "Fill it in on Title Block > Project, or set the project's coordinate system"},
    {"height_datum", "Height datum", Severity::Warning, kProjectTab},
    {"model_name", "Model name", Severity::Info, kProjectTab},
    {"locator_name", "Utilities located by", Severity::Warning, kSignOffTab},
    {"surveyor_name", "Surveyed by", Severity::Warning, kSignOffTab},
    {"compiler_name", "Compiled by", Severity::Warning, kSignOffTab},
    {"reviewer_name", "Reviewed by", Severity::Warning, kSignOffTab},
    {"approver_name", "Approved by", Severity::Warning, kSignOffTab},
    // A sign-off with no date of its own prints the plot date, so these are
    // blank only when that is, and plot_date says so once.
    {"locator_date", "Utilities located on", std::nullopt, kSignOffTab},
    {"surveyor_date", "Surveyed on", std::nullopt, kSignOffTab},
    {"compiler_date", "Compiled on", std::nullopt, kSignOffTab},
    {"reviewer_date", "Reviewed on", std::nullopt, kSignOffTab},
    {"approver_date", "Approved on", std::nullopt, kSignOffTab},
    {"notes", "Notes", std::nullopt, "Fill it in on Title Block > Notes"},
    {"file_name", "File name", Severity::Warning,
     "Save the drawing as a project, or give it a name: the file name is the project folder's"},
    {"sheet_number", "Sheet number", Severity::Warning,
     "Set the numbering on Title Block > Project > Sheet numbering"},
    {"sheet_count", "Sheet count", Severity::Warning, "Clear the sheet's own value for it"},
    {"scale", "Scale", Severity::Warning, "Clear the sheet's own value for it"},
    {"plot_date", "Plot date", Severity::Warning, "Plot with a date"},
    {"revision", "Revision", Severity::Info, "Add a revision on Title Block > Revisions"},
}};

const FieldRule* ruleFor(std::string_view field)
{
    for (const FieldRule& rule : kFieldRules) {
        if (rule.field == field) {
            return &rule;
        }
    }
    return nullptr;
}

// ---- a plan's window on the world -------------------------------------------------------

// The world rectangle a plan viewport shows, in its own axes: `u` runs across
// the paper, `v` up it, each half-size in world units.
struct Window {
    Point2 centre{};
    Vec2 u{1.0, 0.0};
    Vec2 v{0.0, 1.0};
    double halfWidth = 0.0;
    double halfHeight = 0.0;

    [[nodiscard]] Point2 local(const Point2& world) const
    {
        const Vec2 d = world - centre;
        return Point2(d.dot(u), d.dot(v));
    }
    [[nodiscard]] bool containsLocal(const Point2& q) const
    {
        return std::abs(q.x) <= halfWidth && std::abs(q.y) <= halfHeight;
    }
    [[nodiscard]] std::array<Point2, 4> corners() const
    {
        const Vec2 a = u * halfWidth;
        const Vec2 b = v * halfHeight;
        return {centre - a - b, centre + a - b, centre + a + b, centre - a + b};
    }
    [[nodiscard]] Box2 reach() const
    {
        Box2 box;
        for (const Point2& corner : corners()) {
            box.expand(corner);
        }
        return box;
    }
};

Window windowOf(const Viewport& viewport, const PlanWindow& at)
{
    Window window;
    window.centre = at.centre;
    window.u = Vec2(std::cos(viewport.rotation), std::sin(viewport.rotation));
    window.v = Vec2(-window.u.y, window.u.x);
    window.halfWidth = viewport.rect.width() * at.scale / 2000.0;
    window.halfHeight = viewport.rect.height() * at.scale / 2000.0;
    return window;
}

// Whether the segment a-b passes through the window: clipped against it in
// the window's axes (Liang-Barsky).
bool segmentMeets(const Window& window, const Point2& a, const Point2& b)
{
    const Point2 p = window.local(a);
    const Point2 q = window.local(b);
    double t0 = 0.0;
    double t1 = 1.0;
    const Vec2 d = q - p;
    const std::array<std::pair<double, double>, 4> planes{{
        {-d.x, p.x + window.halfWidth},
        {d.x, window.halfWidth - p.x},
        {-d.y, p.y + window.halfHeight},
        {d.y, window.halfHeight - p.y},
    }};
    for (const auto& [denominator, distance] : planes) {
        if (denominator == 0.0) {
            if (distance < 0.0) {
                return false;
            }
            continue;
        }
        const double t = distance / denominator;
        if (denominator < 0.0) {
            t0 = std::max(t0, t);
        } else {
            t1 = std::min(t1, t);
        }
        if (t0 > t1) {
            return false;
        }
    }
    return true;
}

// Whether a world box and the window share any point: the separating-axis
// test on the two rectangles' four axes.
bool boxMeets(const Window& window, const Box2& box)
{
    if (box.empty()) {
        return false;
    }
    if (!box.intersects(window.reach())) {
        return false;
    }
    Box2 local;
    for (const Point2& corner :
         {box.min, box.max, Point2(box.min.x, box.max.y), Point2(box.max.x, box.min.y)}) {
        local.expand(window.local(corner));
    }
    return local.intersects(
        Box2(Point2(-window.halfWidth, -window.halfHeight), Point2(window.halfWidth, window.halfHeight)));
}

// `filled`: the outline is hatched, so a window wholly inside it looks at its
// fill; an outline that is not shows nothing inside it.
bool polylineMeets(const Window& window, const geometry::Polyline2& line, bool filled = false)
{
    const auto& vertices = line.vertices;
    if (vertices.size() == 1) {
        return window.containsLocal(window.local(vertices.front()));
    }
    for (std::size_t i = 1; i < vertices.size(); ++i) {
        if (segmentMeets(window, vertices[i - 1], vertices[i])) {
            return true;
        }
    }
    if (line.closed && vertices.size() > 2) {
        if (segmentMeets(window, vertices.back(), vertices.front())) {
            return true;
        }
        // A window wholly inside a hatched outline is looking at its fill:
        // counted as shown, so a hatched area is never called empty. Inside
        // a bare outline - a boundary, a buffer - there is nothing to see.
        if (!filled) {
            return false;
        }
        bool inside = false;
        const Point2 c = window.centre;
        for (std::size_t i = 0, j = vertices.size() - 1; i < vertices.size(); j = i++) {
            const Point2& a = vertices[i];
            const Point2& b = vertices[j];
            if ((a.y > c.y) != (b.y > c.y) &&
                c.x < (b.x - a.x) * (c.y - a.y) / (b.y - a.y) + a.x) {
                inside = !inside;
            }
        }
        return inside;
    }
    return false;
}

// Whether an entity's drawing reaches into the window. Exact for points,
// lines and polylines; a closed outline or a circle counts when it crosses
// the window, or when the window is inside it and it is hatched (its fill);
// a dimension by everything it draws - its label and arrows reach well past
// the points it measures (queryExtents); arcs, text and anything else by
// their box.
bool entityMeets(const Window& window, const entity::Model& model, const entity::Entity& entity)
{
    const entity::Geometry& geometry = entity.geometry;
    if (const auto* point = std::get_if<entity::PointGeometry>(&geometry)) {
        return window.containsLocal(window.local(point->position));
    }
    if (const auto* segment = std::get_if<geometry::Segment2>(&geometry)) {
        return segmentMeets(window, segment->start, segment->end);
    }
    if (const auto* line = std::get_if<geometry::Polyline2>(&geometry)) {
        return polylineMeets(window, *line,
                             line->closed && resolveHatchPattern(model, entity) != nullptr);
    }
    if (const auto* circle = std::get_if<geometry::Circle2>(&geometry)) {
        const Point2 c = window.local(circle->center);
        const double dx = std::max(std::abs(c.x) - window.halfWidth, 0.0);
        const double dy = std::max(std::abs(c.y) - window.halfHeight, 0.0);
        if (std::hypot(dx, dy) > circle->radius) {
            return false; // the window is wholly outside the circle
        }
        // The farthest corner of the window from the centre: inside the
        // circle, the window sees only its fill.
        const double fx = std::abs(c.x) + window.halfWidth;
        const double fy = std::abs(c.y) + window.halfHeight;
        return std::hypot(fx, fy) >= circle->radius || resolveHatchPattern(model, entity) != nullptr;
    }
    if (std::holds_alternative<entity::DimensionGeometry>(geometry)) {
        return boxMeets(window, katana::cad::detail::queryExtents(model, entity));
    }
    return boxMeets(window, entity::boundingBox(geometry));
}

// ---- everything else --------------------------------------------------------------------

// The findings, and the codes not to report.
class Report {
  public:
    explicit Report(const PreflightOptions& options) : options_(options) {}

    void add(Severity severity, std::string_view code, std::optional<std::size_t> sheetIndex,
             const std::string& sheetId, const std::string& viewportId, std::string subject,
             std::string message, std::string fix)
    {
        if (options_.skip.contains(code)) {
            return;
        }
        findings_.push_back(Finding{severity, std::string(code), sheetIndex, sheetId, viewportId,
                                    std::move(subject), std::move(message), std::move(fix)});
    }

    [[nodiscard]] std::vector<Finding> take() { return std::move(findings_); }

  private:
    const PreflightOptions& options_;
    std::vector<Finding> findings_;
};

// The sheets a check covers: those asked for that exist, in the set's order,
// each once.
std::vector<std::size_t> sheetsToCheck(const SheetSet& set, const PreflightOptions& options)
{
    std::vector<std::size_t> indices;
    if (options.sheets.empty()) {
        for (std::size_t i = 0; i < set.sheets.size(); ++i) {
            indices.push_back(i);
        }
        return indices;
    }
    for (const std::size_t index : options.sheets) {
        if (index < set.sheets.size()) {
            indices.push_back(index);
        }
    }
    std::sort(indices.begin(), indices.end());
    indices.erase(std::unique(indices.begin(), indices.end()), indices.end());
    return indices;
}

// The frame a sheet prints with, when it prints one.
const Frame* printedFrame(const Sheet& sheet, std::map<std::string, Result<Frame>>& frames)
{
    if (sheet.frame.empty() || !sheet.landscape) {
        return nullptr;
    }
    const std::string key = std::format("{}|{}", sheet.frame, static_cast<int>(sheet.paper));
    auto found = frames.find(key);
    if (found == frames.end()) {
        found = frames.emplace(key, frameFor(sheet.frame, sheet.paper, true)).first;
    }
    return found->second ? &*found->second : nullptr;
}

void checkFields(const SheetSet& set, const FieldContext& context,
                 const std::vector<std::size_t>& indices, std::map<std::string, Result<Frame>>& frames,
                 Report& report)
{
    // Per field: on how many framed sheets it prints blank.
    std::map<std::string, std::size_t, std::less<>> blanks;
    std::size_t framed = 0;
    for (const std::size_t index : indices) {
        const Sheet& sheet = set.sheets[index];
        const Frame* frame = printedFrame(sheet, frames);
        if (frame == nullptr) {
            continue;
        }
        ++framed;
        std::set<std::string, std::less<>> printed;
        for (const FrameText& text : frame->texts) {
            printed.insert(text.fields.begin(), text.fields.end());
        }
        const auto values = resolveFields(set, index, context);
        for (const std::string& field : printed) {
            // A value the sheet sets itself, even an empty one, is the user's
            // word on that sheet: a blank there is on purpose.
            if (sheet.fields.contains(field)) {
                continue;
            }
            const auto value = values.find(field);
            if (value == values.end() || isBlank(value->second)) {
                ++blanks[field];
            }
        }
    }
    const auto emit = [&](std::string_view field, std::string_view label, Severity severity,
                          std::string_view fix, std::size_t count) {
        const std::string where =
            count == framed ? std::string(framed == 1 ? "the sheet" : "every sheet")
                            : std::format("{} of {} sheets", count, framed);
        report.add(severity, "field.empty", std::nullopt, {}, {}, std::string(field),
                   std::format("{} is blank in the title block of {}", label, where), std::string(fix));
    };
    // The known fields in the Title Block's order, then any other the frame
    // prints, by name.
    for (const FieldRule& rule : kFieldRules) {
        const auto found = blanks.find(rule.field);
        if (found != blanks.end() && rule.severity) {
            emit(rule.field, rule.label, *rule.severity, rule.fix, found->second);
        }
    }
    for (const auto& [field, count] : blanks) {
        if (ruleFor(field) == nullptr) {
            emit(field, "The field " + field, Severity::Warning,
                 "Fill it in on Title Block, or on the sheet's own title-block values", count);
        }
    }
}

void checkLogo(const SheetSet& set, const std::vector<std::size_t>& indices,
               std::map<std::string, Result<Frame>>& frames, const PreflightOptions& options,
               Report& report)
{
    const bool framed = std::any_of(indices.begin(), indices.end(), [&](std::size_t index) {
        const Frame* frame = printedFrame(set.sheets[index], frames);
        return frame != nullptr && frame->cell("logo") != nullptr;
    });
    if (!framed) {
        return;
    }
    const std::string& logo = set.defaults.logoAsset;
    if (logo.empty()) {
        report.add(Severity::Info, "logo.missing", std::nullopt, {}, {}, {},
                   "The title block's logo slot is empty",
                   "Choose your organisation's logo on Title Block > Logo (save the project first: "
                   "the logo is kept in it)");
    } else if (options.logoReadable == false) {
        report.add(Severity::Warning, "logo.unreadable", std::nullopt, {}, {}, logo,
                   std::format("The logo {} is not in the project's assets folder or cannot be read: "
                               "its slot prints empty",
                               logo),
                   "Choose the logo again on Title Block > Logo");
    }
}

void checkSheetItself(const SheetSet& set, std::size_t index, std::map<std::string, Result<Frame>>& frames,
                      Report& report)
{
    const Sheet& sheet = set.sheets[index];
    const auto add = [&](Severity severity, std::string_view code, std::string subject,
                         std::string message, std::string fix) {
        report.add(severity, code, index, sheet.id, {}, std::move(subject), std::move(message),
                   std::move(fix));
    };

    if (sheet.id.empty()) {
        add(Severity::Error, "sheet.duplicate-id", {}, "The sheet has no id: nothing can lead to it",
            "Remove the sheet and add it again, or duplicate it: a new sheet gets an id");
    } else {
        for (std::size_t other = 0; other < index; ++other) {
            if (set.sheets[other].id == sheet.id) {
                add(Severity::Error, "sheet.duplicate-id", set.sheets[other].id,
                    std::format("The sheet has the same id, {}, as sheet {}: match lines lead to the "
                                "first of them",
                                sheet.id, other + 1),
                    "Duplicate the sheet and remove this one: the copy gets an id of its own");
                break;
            }
        }
    }

    if (const std::string name = trimmedUpper(sheet.name); !name.empty()) {
        for (std::size_t other = 0; other < index; ++other) {
            if (trimmedUpper(set.sheets[other].name) == name) {
                add(Severity::Warning, "sheet.duplicate-name", set.sheets[other].id,
                    std::format("The sheet has the same name as sheet {}: {}", other + 1, sheet.name),
                    "Rename one of them: a sheet is found by its name");
                break;
            }
        }
    }

    if (!sheet.frame.empty() && sheet.landscape && printedFrame(sheet, frames) == nullptr) {
        add(Severity::Error, "frame.unknown", sheet.frame,
            std::format("The sheet asks for the frame {}, which this build does not have: it prints "
                        "without a title block",
                        sheet.frame),
            "Choose the built-in frame in the sheet's properties");
    }
    if (!sheet.frame.empty() && !sheet.landscape) {
        add(Severity::Warning, "portrait.no-frame", sheet.frame,
            "The sheet is portrait, so it prints without its title block: the frame is laid out for "
            "landscape paper",
            "Make the sheet landscape, or turn its frame off");
    }
    if (sheet.viewports.empty()) {
        add(Severity::Warning, "sheet.empty", {},
            sheet.frame.empty() || !sheet.landscape ? "The sheet has no views: it prints a blank page"
                                                    : "The sheet has no views: it prints an empty frame",
            "Add a view, or remove the sheet");
    }
}

// What a plan viewport shows once its window is known: whether anything it
// draws is in the window, and its text too small to read.
struct PlanContent {
    bool shown = false;
    std::size_t smallTexts = 0;
    double smallestModel = 0.0; // model units
};

PlanContent planContent(const Viewport& viewport, const Window& window, double scale,
                        const entity::Model& model, const PreflightOptions& options)
{
    PlanContent content;
    const double minimumModel = options.minimumTextMm * scale / 1000.0;
    std::vector<geometry::SpatialId> scratch;
    katana::cad::detail::forEachCandidate(model, options.index, window.reach(), scratch, [&](const entity::Entity& e) {
        if (!isDrawn(model, e, viewport.hiddenLayers) || !entityMeets(window, model, e)) {
            return;
        }
        content.shown = true;
        double height = 0.0;
        if (const auto* text = std::get_if<entity::TextGeometry>(&e.geometry)) {
            if (isBlank(text->text)) {
                return;
            }
            height = text->height;
        } else if (std::holds_alternative<entity::DimensionGeometry>(e.geometry)) {
            height = resolveDimensionStyle(model, e).textHeight;
        } else {
            return;
        }
        if (height < minimumModel) {
            content.smallestModel =
                content.smallTexts == 0 ? height : std::min(content.smallestModel, height);
            ++content.smallTexts;
        }
    });
    if (!content.shown) {
        model.alignments.forEach([&](const entity::Alignment& alignment) {
            if (content.shown) {
                return;
            }
            if (auto solved = geometry::solveAlignment(alignment.horizontal)) {
                content.shown = polylineMeets(window, solved->toPolyline(0.01));
            }
        });
    }
    if (!content.shown) {
        content.shown = std::any_of(options.otherContent.begin(), options.otherContent.end(),
                                    [&](const Box2& box) { return boxMeets(window, box); });
    }
    // A plan's match lines are drawn too.
    if (!content.shown) {
        content.shown = std::any_of(viewport.marks.begin(), viewport.marks.end(),
                                    [&](const WorldMark& mark) {
                                        // A key plan's stored outlines are
                                        // stale copies the painter no longer
                                        // draws; its live ones are counted
                                        // by checkPlan.
                                        if (viewport.kind == ViewportKind::KeyPlan &&
                                            mark.kind == WorldMark::Kind::SheetOutline) {
                                            return false;
                                        }
                                        // Its lines only: an outline is not filled.
                                        geometry::Polyline2 line{mark.points, false};
                                        if (mark.kind == WorldMark::Kind::SheetOutline &&
                                            line.vertices.size() > 2) {
                                            line.vertices.push_back(line.vertices.front());
                                        }
                                        return !line.vertices.empty() && polylineMeets(window, line);
                                    });
    }
    return content;
}

void checkPlan(const SheetSet& set, const Viewport& viewport, std::size_t index, const Sheet& sheet,
               const entity::Model& model, const PreflightOptions& options, Report& report)
{
    const auto add = [&](Severity severity, std::string_view code, std::string subject,
                         std::string message, std::string fix) {
        report.add(severity, code, index, sheet.id, viewport.id, std::move(subject),
                   std::move(message), std::move(fix));
    };
    const std::string label = labelOf(viewport);
    const std::string& alignment = viewport.source.alignment;
    if (!alignment.empty() && model.alignments.find(alignment) == nullptr) {
        add(Severity::Warning, "plan.alignment-missing", alignment,
            std::format("{} follows the alignment {}, which the drawing does not have", label, alignment),
            "Generate the sheet again from an alignment the drawing has, or clear the view's alignment");
    }
    PlanWindow at =
        options.resolvePlan ? options.resolvePlan(viewport) : planWindow(viewport, model, options.otherContent);
    // A key plan is drawn live (key_plan.hpp): the sheets' plans outlined
    // where they are now, each automatic one placed as the painter places it,
    // and an automatic key plan fitted to those outlines rather than to the
    // drawing - so it is checked at the window it is drawn at, and what it
    // shows includes the outlines.
    std::vector<KeyPlanOutline> outlines;
    if (viewport.kind == ViewportKind::KeyPlan) {
        const PlanPlacer place = options.resolvePlan
                                     ? PlanPlacer([&options](const Viewport& plan) {
                                           const PlanWindow placed = options.resolvePlan(plan);
                                           return PlanPlacement{placed.scale, placed.centre};
                                       })
                                     : modelPlacer(model);
        outlines = keyPlanOutlines(set, index, place);
        if (!outlines.empty() && (viewport.autoScale || viewport.autoCentre)) {
            const PlanPlacement fitted = fitKeyPlan(viewport, outlines, Box2{});
            at = PlanWindow{fitted.scale, fitted.centre};
        }
    }
    if (!(at.scale > 0.0) || !std::isfinite(at.scale) || !std::isfinite(at.centre.x) ||
        !std::isfinite(at.centre.y)) {
        // A fixed scale that cannot be used has had scale.invalid. An
        // automatic one falls back to the stored scale and centre when there
        // is nothing to fit, and those cannot be used either: say so here,
        // since scale.invalid looks only at fixed scales.
        if (viewport.autoScale) {
            add(Severity::Error, "scale.invalid", {},
                std::format("{} has nothing to fit its automatic scale to, and its stored scale ({}) "
                            "cannot be used",
                            label, at.scale),
                "Give it a scale, or draw something for it to show");
        }
        return;
    }
    const Window window = windowOf(viewport, at);
    PlanContent content = planContent(viewport, window, at.scale, model, options);
    if (!content.shown) {
        content.shown = std::any_of(outlines.begin(), outlines.end(), [&](const KeyPlanOutline& outline) {
            geometry::Polyline2 ring{outline.corners, false};
            if (!ring.vertices.empty()) {
                ring.vertices.push_back(ring.vertices.front());
            }
            return ring.vertices.size() > 1 && polylineMeets(window, ring);
        });
    }
    if (!content.shown) {
        add(Severity::Warning, "plan.empty", {},
            std::format("{} shows nothing: nothing it draws lies in its {:.1f} x {:.1f} m window at {} "
                        "about ({:.3f}, {:.3f})",
                        label, 2.0 * window.halfWidth, 2.0 * window.halfHeight, scaleName(at.scale),
                        at.centre.x, at.centre.y),
            "Pan the drawing into it (Shift-drag), set its scale and centre to Auto, or show the layers "
            "it hides");
    }
    // The coordinate grid, worked out as the painter works it out at this
    // window (plan_grid.hpp).
    if (viewport.gridStyle != GridStyle::None) {
        const auto grid = planGrid(viewport, PlanPlacement{at.scale, at.centre});
        if (!grid) {
            add(Severity::Error, "grid.invalid", {},
                std::format("{}'s coordinate grid is not drawn: {}", label, grid.error().message),
                std::format("Set its grid interval to Auto, or to at least {:.3g} m at {}",
                            kGridMinimumSpacingMm * at.scale / 1000.0, scaleName(at.scale)));
        } else if (grid->lines.empty()) {
            add(Severity::Info, "grid.empty", {},
                std::format("{}'s coordinate grid, every {:.6g} m, has no line in its window", label,
                            grid->interval),
                "Set its grid interval to Auto, or make it smaller");
        }
    }
    // A key plan is a small-scale map of where the sheets are: its drawing
    // is faded and its own text is not meant to be read.
    if (content.smallTexts > 0 && viewport.kind != ViewportKind::KeyPlan) {
        const double smallestMm = content.smallestModel * 1000.0 / at.scale;
        // The largest standard scale at which the smallest text reaches the
        // minimum, and the text height that reads at this one.
        const double ceiling = content.smallestModel * 1000.0 / options.minimumTextMm;
        std::optional<double> readable;
        for (const double s : kSheetScales) {
            if (s <= ceiling) {
                readable = s;
            }
        }
        const std::string height = std::format("{:.3g} m", options.minimumTextMm * at.scale / 1000.0);
        std::string fix = readable ? std::format("Plot the view at {} or larger, or make the text at least "
                                                 "{} high",
                                                 scaleName(*readable), height)
                                   : std::format("Make the text at least {} high", height);
        add(Severity::Warning, "text.too-small", {},
            std::format("{} has {} text{} printing smaller than {} at {}, the smallest {:.2f} mm", label,
                        content.smallTexts, content.smallTexts == 1 ? "" : "s", mm(options.minimumTextMm),
                        scaleName(at.scale), smallestMm),
            std::move(fix));
    }
}

// The chainages a cross-section viewport cuts, as the painter lists them.
std::vector<double> crossStations(const ViewportSource& from)
{
    std::vector<double> stations = from.stations;
    if (stations.empty() && from.sectionInterval > 0.0 && from.chainageTo > from.chainageFrom) {
        for (double at = from.chainageFrom; at <= from.chainageTo + 1e-9 && stations.size() < 64;
             at += from.sectionInterval) {
            stations.push_back(at);
        }
    }
    return stations;
}

void checkSection(const Viewport& viewport, std::size_t index, const Sheet& sheet,
                  const entity::Model& model, const PreflightOptions& options, Report& report)
{
    const auto add = [&](Severity severity, std::string_view code, std::string subject,
                         std::string message, std::string fix) {
        report.add(severity, code, index, sheet.id, viewport.id, std::move(subject),
                   std::move(message), std::move(fix));
    };
    const std::string label = labelOf(viewport);
    const ViewportSource& from = viewport.source;
    const bool cross = viewport.kind == ViewportKind::CrossSections;
    if (from.alignment.empty()) {
        add(Severity::Error, "section.alignment-missing", {},
            std::format("{} has no alignment: it prints empty", label),
            "Choose its alignment in the view's properties");
        return;
    }
    const entity::Alignment* alignment = model.alignments.find(from.alignment);
    if (alignment == nullptr) {
        add(Severity::Error, "section.alignment-missing", from.alignment,
            std::format("{} cuts the alignment {}, which the drawing does not have: it prints empty",
                        label, from.alignment),
            "Choose another alignment in the view's properties, or remove the view");
        return;
    }
    const auto solved = geometry::solveAlignment(alignment->horizontal);
    if (!solved) {
        add(Severity::Error, "section.alignment-invalid", from.alignment,
            std::format("{} cuts the alignment {}, which cannot be solved: {}", label, from.alignment,
                        solved.error().describe()),
            "Mend the alignment's geometry");
        return;
    }
    const std::string runs = std::format("{} runs {} to {}", from.alignment,
                                         chainage(solved->startStation()), chainage(solved->endStation()));
    if (cross) {
        const std::vector<double> stations = crossStations(from);
        if (stations.empty()) {
            add(Severity::Error, "section.no-stations", from.alignment,
                std::format("{} has no chainage to cut a section at: it prints empty", label),
                "Give it chainages, or an interval and a range, in the view's properties");
        } else {
            const double halfWidth = from.sectionHalfWidth > 0.0 ? from.sectionHalfWidth : 20.0;
            std::vector<double> off;
            for (const double station : stations) {
                if (!solved->pointAtStationOffset(station, halfWidth) ||
                    !solved->pointAtStationOffset(station, -halfWidth)) {
                    off.push_back(station);
                }
            }
            if (!off.empty()) {
                std::string listed;
                for (std::size_t i = 0; i < off.size() && i < 3; ++i) {
                    listed += (i == 0 ? "" : ", ") + chainage(off[i]);
                }
                if (off.size() > 3) {
                    listed += std::format(" and {} more", off.size() - 3);
                }
                add(Severity::Error, "section.station-outside", from.alignment,
                    std::format("{} cuts {} of its {} section{} off the alignment ({}): they print "
                                "blank. {}",
                                label, off.size(), stations.size(), stations.size() == 1 ? "" : "s",
                                listed, runs),
                    std::format("Cut them between {} and {}", chainage(solved->startStation()),
                                chainage(solved->endStation())));
            }
        }
    } else if (from.chainageTo > from.chainageFrom) {
        const double start = solved->startStation();
        const double end = solved->endStation();
        constexpr double kSlack = 1e-6;
        if (from.chainageFrom < start - kSlack || from.chainageTo > end + kSlack) {
            const bool none = from.chainageTo <= start || from.chainageFrom >= end;
            add(none ? Severity::Error : Severity::Warning, "section.station-outside", from.alignment,
                std::format("{} shows {} to {}, {}: {}", label, chainage(from.chainageFrom),
                            chainage(from.chainageTo), none ? "all of it off the alignment" : "running past its end",
                            runs + (none ? "; it prints empty" : "; the rest is blank")),
                std::format("Set its range within {} to {}", chainage(start), chainage(end)));
        }
    }
    if (options.sectionSurfaces && *options.sectionSurfaces == 0 && (cross || !alignment->vertical)) {
        add(Severity::Error, "section.no-surface", from.alignment,
            cross ? std::format("{} has no surface to cut: it prints empty", label)
                  : std::format("{} has no surface to cut, and {} has no design profile: it prints empty",
                                label, from.alignment),
            "Build a surface (Terrain > Surface From ...), or show a hidden one");
    }
}

void checkViewport(const SheetSet& set, std::size_t index, std::size_t position,
                   const std::map<std::string, std::pair<std::size_t, std::size_t>, std::less<>>& firstIds,
                   std::map<std::string, Result<Frame>>& frames, const entity::Model& model,
                   const PreflightOptions& options, Report& report)
{
    const Sheet& sheet = set.sheets[index];
    const Viewport& viewport = sheet.viewports[position];
    const auto add = [&](Severity severity, std::string_view code, std::string subject,
                         std::string message, std::string fix) {
        report.add(severity, code, index, sheet.id, viewport.id, std::move(subject),
                   std::move(message), std::move(fix));
    };
    const std::string label = labelOf(viewport);

    if (viewport.id.empty()) {
        add(Severity::Error, "viewport.duplicate-id", {}, "A viewport has no id: it cannot be edited",
            "Remove it and add it again: a new view gets an id");
    } else if (const auto first = firstIds.find(viewport.id);
               first != firstIds.end() && first->second != std::pair{index, position}) {
        add(Severity::Error, "viewport.duplicate-id", viewport.id,
            std::format("{} has the same id as a view on sheet {}: an edit of one may change the other",
                        label, first->second.first + 1),
            "Duplicate its sheet and remove this one: the copy's views get ids of their own");
    }

    if (viewport.rect.empty()) {
        add(Severity::Warning, "viewport.unplaced", {},
            std::format("{} is not placed on the paper, so it does not print", label),
            "Tile the sheet, or give the view a rectangle");
        return;
    }

    // On the paper, inside the drawing area, clear of the title block.
    const PaperDimensions paperSize = paperDimensions(sheet.paper, sheet.landscape);
    const Box2 paper(Point2(0.0, 0.0), Point2(paperSize.widthMm, paperSize.heightMm));
    const Box2& rect = viewport.rect;
    const Box2 area = drawingArea(sheet);
    const double tolerance = options.outsideToleranceMm;
    const auto beyond = [](const Box2& inner, const Box2& outer) {
        return std::max({outer.min.x - inner.min.x, inner.max.x - outer.max.x, outer.min.y - inner.min.y,
                         inner.max.y - outer.max.y, 0.0});
    };
    const auto shared = [](const Box2& a, const Box2& b) {
        return std::pair{std::min(a.max.x, b.max.x) - std::max(a.min.x, b.min.x),
                         std::min(a.max.y, b.max.y) - std::max(a.min.y, b.min.y)};
    };
    const Frame* frame = printedFrame(sheet, frames);
    const auto [underWidth, underHeight] =
        frame != nullptr ? shared(rect, frame->titleBlock) : std::pair{0.0, 0.0};
    const std::string back = "Drag it back inside the drawing area, or tile the sheet";
    if (!rect.intersects(paper)) {
        add(Severity::Error, "viewport.outside", {}, std::format("{} is off the paper: it does not print", label),
            back);
    } else if (const double off = beyond(rect, paper); off > tolerance) {
        add(Severity::Error, "viewport.outside", {},
            std::format("{} runs {} off the paper: that part is cut off", label, mm(off)), back);
    } else if (underWidth > tolerance && underHeight > tolerance) {
        add(Severity::Error, "viewport.outside", {},
            std::format("{} runs {} under the title block: the title block covers it", label,
                        mm(underHeight)),
            back);
    } else if (const double out = beyond(rect, area); out > tolerance) {
        add(Severity::Warning, "viewport.outside", {},
            std::format("{} runs {} outside the drawing area, into the margin", label, mm(out)), back);
    }

    // What it covers of the views behind it.
    for (std::size_t other = 0; other < position; ++other) {
        const Viewport& under = sheet.viewports[other];
        if (under.rect.empty()) {
            continue;
        }
        const auto [w, h] = shared(rect, under.rect);
        if (w <= options.overlapToleranceMm || h <= options.overlapToleranceMm) {
            continue;
        }
        const bool inset = isPanel(viewport.kind) && !isPanel(under.kind) && under.rect.contains(rect);
        add(inset ? Severity::Info : Severity::Warning, "viewport.overlap", under.id,
            inset ? std::format("{} is set inside {}: the drawing under it does not print", label,
                                labelOf(under))
                  : std::format("{} covers {:.1f} x {} of {}: what is under it does not print", label, w,
                                mm(h), labelOf(under)),
            inset ? "Move it where the drawing under it is not needed"
                  : "Move or resize one of them, or tile the sheet");
    }

    if (const SizeMm least = minimumSize(viewport.kind);
        rect.width() < least.width - tolerance || rect.height() < least.height - tolerance) {
        add(Severity::Info, "viewport.too-small", {},
            std::format("{} is {:.1f} x {}, smaller than {:.0f} x {:.0f} mm, the least a {} view reads at",
                        label, rect.width(), mm(rect.height()), least.width, least.height,
                        std::string(toString(viewport.kind))),
            "Make it larger");
    }

    if (isScaled(viewport.kind) && !viewport.autoScale) {
        const double scale = viewport.scale;
        if (!(scale > 0.0) || !std::isfinite(scale)) {
            add(Severity::Error, "scale.invalid", {},
                std::format("{} has no usable scale ({})", label, scale),
                "Give it a scale, or set it to Auto");
        } else if (std::none_of(kSheetScales.begin(), kSheetScales.end(),
                                [scale](double s) { return std::abs(s - scale) <= 1e-9 * s; })) {
            std::string fix;
            const auto above = std::find_if(kSheetScales.begin(), kSheetScales.end(),
                                            [scale](double s) { return s > scale; });
            if (above == kSheetScales.begin()) {
                fix = std::format("Use {}", scaleName(*above));
            } else if (above == kSheetScales.end()) {
                fix = std::format("Use {} if it fits", scaleName(kSheetScales.back()));
            } else {
                fix = std::format("Use {} (it shows a little more) or {}", scaleName(*above),
                                  scaleName(*(above - 1)));
            }
            add(Severity::Info, "scale.non-standard", {},
                std::format("{} is at 1:{}, which is not a standard scale", label,
                            std::format("{:.6g}", scale)),
                std::move(fix));
        }
    }

    switch (viewport.kind) {
    case ViewportKind::Plan:
    case ViewportKind::KeyPlan: // drawn as a plan is, so checked as one
        checkPlan(set, viewport, index, sheet, model, options, report);
        break;
    case ViewportKind::LongSection:
    case ViewportKind::CrossSections:
        checkSection(viewport, index, sheet, model, options, report);
        break;
    case ViewportKind::Image:
        if (viewport.text.empty()) {
            add(Severity::Error, "image.missing", {}, std::format("{} names no image: it prints empty", label),
                "Choose its image in the view's properties");
        } else if (!options.assets.empty()) {
            // Where the painter looks for it.
            std::error_code error;
            if (!std::filesystem::is_regular_file(options.assets / viewport.text, error)) {
                add(Severity::Error, "image.missing", viewport.text,
                    std::format("{} shows {}, which is not in the project's assets folder: it prints empty",
                                label, viewport.text),
                    "Choose its image again in the view's properties");
            }
        }
        break;
    case ViewportKind::Notes:
        if (isBlank(viewport.text)) {
            add(Severity::Info, "notes.empty", {}, std::format("{} has no text: it prints an empty box", label),
                "Type its text in the view's properties, or remove it");
        }
        break;
    case ViewportKind::SheetIndex:
    case ViewportKind::Revisions: {
        // A table is laid out as the painter lays it out (tables.hpp), so
        // the rows it leaves out are the rows the plot leaves out: a register
        // that says "+3 more" hands over a set whose last sheets it never
        // lists.
        const bool revisions = viewport.kind == ViewportKind::Revisions;
        if (revisions && set.revisions.empty()) {
            add(Severity::Info, "revisions.empty", {},
                std::format("{} has no revisions to list: it prints its headings alone", label),
                "Add a revision on the Title Block, or remove the table");
            break;
        }
        const TableLayout table =
            layoutViewportTable(options.drawnSet != nullptr ? *options.drawnSet : set, index, viewport);
        if (table.rowsHidden > 0) {
            add(Severity::Warning, "table.overflow", {},
                std::format("{} shows {} of its {} rows: {} do not fit and print as \"+{} {}\"",
                            label, table.rowsShown, table.rowsShown + table.rowsHidden,
                            table.rowsHidden, table.rowsHidden, revisions ? "earlier" : "more"),
                revisions ? "Make it taller, or show only the newest revisions"
                          : "Make it larger, or give it a sheet of its own");
        }
        break;
    }
    case ViewportKind::Legend: {
        // Gathered and flowed as the painter gathers and flows it (legend.hpp),
        // the labels measured by Arial's widths as the tables are.
        LegendOptions gather;
        gather.index = options.index;
        if (options.resolvePlan) {
            gather.window = options.resolvePlan;
        }
        const auto legend = computeLegend(model, set, index, viewport.legendScope, gather);
        if (!legend) {
            break;
        }
        if (legend->entries.empty()) {
            add(Severity::Info, "legend.empty", std::string(toString(viewport.legendScope)),
                std::format("{} lists nothing: the plans it reads ({}) show nothing", label,
                            toString(viewport.legendScope)),
                "Widen its scope to the whole set or drawing, or remove it");
            break;
        }
        std::vector<double> widths;
        for (const LegendEntry& entry : legend->entries) {
            widths.push_back(
                estimateTextWidth(katana::core::uppered(entry.label), 1.8, false) * 0.9);
        }
        if (const LegendLayout flowed = layoutLegend(viewport.rect, widths); flowed.more > 0) {
            add(Severity::Warning, "legend.overflow", {},
                std::format("{} lists {} of its {} entries: {} do not fit and print as \"+{} more\"",
                            label, legend->entries.size() - flowed.more, legend->entries.size(),
                            flowed.more, flowed.more),
                "Make it larger, or narrow its scope to this sheet");
        }
        break;
    }
    case ViewportKind::Model3D:
        break;
    }

    // Marks leading to sheets that were removed.
    std::size_t lines = 0;
    std::size_t outlines = 0;
    std::vector<std::string> gone;
    for (const WorldMark& mark : viewport.marks) {
        if (mark.sheet.empty() || sheetIndex(set, mark.sheet)) {
            continue;
        }
        ++(mark.kind == WorldMark::Kind::MatchLine ? lines : outlines);
        if (std::find(gone.begin(), gone.end(), mark.sheet) == gone.end()) {
            gone.push_back(mark.sheet);
        }
    }
    if (!gone.empty()) {
        std::string ids;
        for (const std::string& id : gone) {
            ids += (ids.empty() ? "" : ", ") + id;
        }
        std::string what;
        if (lines > 0) {
            what = std::format("{} match line{}", lines, lines == 1 ? "" : "s");
        }
        if (outlines > 0) {
            what += std::format("{}{} key-plan outline{}", what.empty() ? "" : " and ", outlines,
                                outlines == 1 ? "" : "s");
        }
        add(Severity::Warning, "matchline.dangling", ids,
            std::format("{} has {} leading to sheets that no longer exist ({}): they print their label alone",
                        label, what, ids),
            "Generate the sheets again, or remove the marks");
    }
}

std::string severityKey(Severity severity)
{
    return std::string(toString(severity));
}

} // namespace

std::string_view toString(Severity severity)
{
    switch (severity) {
    case Severity::Error:
        return "error";
    case Severity::Warning:
        return "warning";
    case Severity::Info:
        return "info";
    }
    return "warning";
}

std::optional<Severity> severityFrom(std::string_view name)
{
    for (const Severity severity : {Severity::Error, Severity::Warning, Severity::Info}) {
        if (toString(severity) == name) {
            return severity;
        }
    }
    return std::nullopt;
}

std::span<const CheckDescription> preflightChecks()
{
    return kChecks;
}

PlanWindow planWindow(const Viewport& viewport, const entity::Model& model,
                      std::span<const Box2> otherContent)
{
    // The painter's rule (resolvePlanViewport), from the model alone.
    PlanWindow at{viewport.scale, viewport.centre};
    if ((!viewport.autoScale && !viewport.autoCentre) || viewport.rect.empty()) {
        return at;
    }
    std::vector<Point2> points;
    const ViewportSource& from = viewport.source;
    if (!from.alignment.empty()) {
        if (const auto* alignment = model.alignments.find(from.alignment)) {
            if (auto solved = geometry::solveAlignment(alignment->horizontal)) {
                double a = from.chainageFrom;
                double b = from.chainageTo;
                if (!(b > a)) {
                    a = solved->startStation();
                    b = solved->endStation();
                }
                for (int i = 0; i <= 64; ++i) {
                    if (auto p = solved->pointAtStation(a + (b - a) * i / 64.0)) {
                        points.push_back(*p);
                    }
                }
            }
        }
    }
    if (points.empty()) {
        Box2 box = drawnExtent(model, viewport.hiddenLayers);
        model.alignments.forEach([&box](const entity::Alignment& alignment) {
            if (const auto solved = geometry::solveAlignment(alignment.horizontal)) {
                for (const Point2& vertex : solved->toPolyline(1.0).vertices) {
                    box.expand(vertex);
                }
            }
        });
        for (const Box2& other : otherContent) {
            box.expand(other);
        }
        if (box.empty()) {
            return at;
        }
        points = {box.min, box.max, Point2(box.min.x, box.max.y), Point2(box.max.x, box.min.y)};
    }
    if (viewport.autoCentre) {
        Box2 box;
        for (const Point2& p : points) {
            box.expand(p.rotated(-viewport.rotation));
        }
        at.centre = box.center().rotated(viewport.rotation);
    }
    if (viewport.autoScale) {
        double halfW = 0.0;
        double halfH = 0.0;
        for (const Point2& p : points) {
            const Point2 d = (p - at.centre).rotated(-viewport.rotation);
            halfW = std::max(halfW, std::abs(d.x));
            halfH = std::max(halfH, std::abs(d.y));
        }
        const double needed = std::max(2.0 * halfW * 1000.0 / viewport.rect.width(),
                                       2.0 * halfH * 1000.0 / viewport.rect.height()) *
                              1.04;
        if (needed > 0.0) {
            if (auto scale = sheetScaleAtLeast(needed)) {
                at.scale = *scale;
            }
        }
    }
    return at;
}

std::array<Point2, 4> planWindowCorners(const Viewport& viewport, const PlanWindow& window)
{
    return windowOf(viewport, window).corners();
}

std::vector<Finding> checkSheets(const SheetSet& set, const entity::Model& model,
                                 const FieldContext& context, const PreflightOptions& options)
{
    Report report(options);
    const std::vector<std::size_t> indices = sheetsToCheck(set, options);
    std::map<std::string, Result<Frame>> frames;

    // The set's own findings first: they are about every sheet.
    checkFields(set, context, indices, frames, report);
    checkLogo(set, indices, frames, options, report);
    if (const core::Status setup = validatePageSetup(set.pageSetup); !setup) {
        report.add(Severity::Error, "pagesetup.invalid", std::nullopt, {}, {}, {},
                   std::format("The page setup cannot be plotted: {}", setup.error().message),
                   "Correct it in the Plot dialog, or with SHEETS PAGESETUP");
    }

    // Where each viewport id is first used, over the whole set.
    std::map<std::string, std::pair<std::size_t, std::size_t>, std::less<>> firstIds;
    for (std::size_t s = 0; s < set.sheets.size(); ++s) {
        for (std::size_t v = 0; v < set.sheets[s].viewports.size(); ++v) {
            firstIds.try_emplace(set.sheets[s].viewports[v].id, s, v);
        }
    }
    for (const std::size_t index : indices) {
        checkSheetItself(set, index, frames, report);
        for (std::size_t position = 0; position < set.sheets[index].viewports.size(); ++position) {
            checkViewport(set, index, position, firstIds, frames, model, options, report);
        }
    }
    return report.take();
}

std::vector<Finding> checkDocumentSheets(const Document& document, PreflightOptions options)
{
    const SheetSet& set = document.sheetSet();
    if (options.index == nullptr) {
        options.index = &document.spatialIndex();
    }
    if (!options.logoReadable && !set.defaults.logoAsset.empty()) {
        const auto path = logoPath(document);
        std::error_code error;
        options.logoReadable = path && std::filesystem::is_regular_file(*path, error);
    }
    if (options.assets.empty()) {
        if (const auto directory = document.projectDirectory()) {
            options.assets = *directory / "assets";
        }
    }
    // Any date will do: the findings ask only whether a date prints.
    const auto today = std::chrono::year_month_day(
        std::chrono::floor<std::chrono::days>(std::chrono::system_clock::now()));
    return checkSheets(set, document.model(), fieldContextFor(document, frameDate(today)), options);
}

std::vector<Finding> findingsOnSheets(std::span<const Finding> findings,
                                      std::span<const std::size_t> indices)
{
    std::vector<Finding> kept;
    for (const Finding& finding : findings) {
        if (indices.empty() || !finding.sheetIndex ||
            std::find(indices.begin(), indices.end(), *finding.sheetIndex) != indices.end()) {
            kept.push_back(finding);
        }
    }
    return kept;
}

PreflightSummary summarize(std::span<const Finding> findings)
{
    PreflightSummary summary;
    for (const Finding& finding : findings) {
        switch (finding.severity) {
        case Severity::Error:
            ++summary.errors;
            break;
        case Severity::Warning:
            ++summary.warnings;
            break;
        case Severity::Info:
            ++summary.infos;
            break;
        }
    }
    return summary;
}

std::string summaryText(const PreflightSummary& summary)
{
    std::string text;
    const auto part = [&text](std::size_t count, std::string_view one, std::string_view many) {
        if (count > 0) {
            text += std::format("{}{} {}", text.empty() ? "" : ", ", count, count == 1 ? one : many);
        }
    };
    part(summary.errors, "error", "errors");
    part(summary.warnings, "warning", "warnings");
    part(summary.infos, "note", "notes");
    return text.empty() ? std::string("no problems found") : text;
}

std::string findingLine(const Finding& finding)
{
    const std::string severity = katana::core::uppered(toString(finding.severity));
    std::string where;
    if (!finding.sheetId.empty() || !finding.viewportId.empty()) {
        where = " [" + finding.sheetId + (finding.viewportId.empty() ? "" : "/" + finding.viewportId) + "]";
    } else if (finding.sheetIndex) {
        where = std::format(" [sheet {}]", *finding.sheetIndex + 1);
    }
    std::string line = severity + " " + finding.code + where + " " + finding.message + ".";
    if (!finding.fix.empty()) {
        line += " Fix: " + finding.fix + ".";
    }
    return line;
}

std::string findingsToJson(std::span<const Finding> findings)
{
    const PreflightSummary summary = summarize(findings);
    Json list = Json::array();
    for (const Finding& finding : findings) {
        Json item = Json::object();
        item["severity"] = severityKey(finding.severity);
        item["code"] = finding.code;
        if (finding.sheetIndex) {
            item["sheet_index"] = *finding.sheetIndex;
        }
        const std::pair<std::string_view, const std::string*> texts[] = {
            {"sheet", &finding.sheetId},   {"viewport", &finding.viewportId},
            {"subject", &finding.subject}, {"message", &finding.message},
            {"fix", &finding.fix},
        };
        for (const auto& [key, value] : texts) {
            if (!value->empty()) {
                item[std::string(key)] = *value;
            }
        }
        list.push_back(std::move(item));
    }
    Json json = Json::object();
    json["format"] = "katana-sheet-checks";
    json["version"] = 1;
    json["summary"] = {{"errors", summary.errors}, {"warnings", summary.warnings}, {"info", summary.infos}};
    json["findings"] = std::move(list);
    return json.dump();
}

Result<std::vector<Finding>> findingsFromJson(std::string_view text)
{
    try {
        const Json json = Json::parse(text);
        if (!json.is_object() || json.value("format", std::string{}) != "katana-sheet-checks") {
            return makeError(ErrorCode::ParseFailure, "not a sheet-checks report");
        }
        if (json.value("version", 0) != 1) {
            return makeError(ErrorCode::Unsupported, "a sheet-checks report of another version",
                             std::to_string(json.value("version", 0)));
        }
        const Json& list = json.at("findings");
        if (!list.is_array()) {
            return makeError(ErrorCode::ParseFailure, "a sheet-checks report whose findings are not a list");
        }
        std::vector<Finding> findings;
        for (const Json& item : list) {
            Finding finding;
            const auto severity = severityFrom(item.at("severity").get<std::string>());
            if (!severity) {
                return makeError(ErrorCode::ParseFailure, "an unknown severity",
                                 item.at("severity").get<std::string>());
            }
            finding.severity = *severity;
            finding.code = item.at("code").get<std::string>();
            if (item.contains("sheet_index")) {
                // A position: a negative or fractional one is not read as a
                // huge or rounded one.
                if (!item.at("sheet_index").is_number_unsigned()) {
                    return makeError(ErrorCode::ParseFailure, "a sheet index that is not a position",
                                     item.at("sheet_index").dump());
                }
                finding.sheetIndex = item.at("sheet_index").get<std::size_t>();
            }
            finding.sheetId = item.value("sheet", std::string{});
            finding.viewportId = item.value("viewport", std::string{});
            finding.subject = item.value("subject", std::string{});
            finding.message = item.value("message", std::string{});
            finding.fix = item.value("fix", std::string{});
            findings.push_back(std::move(finding));
        }
        return findings;
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "a sheet-checks report that cannot be read", error.what());
    }
}

} // namespace katana::cad::plotting
