#include "plotting/sheet_arrange.hpp"

#include <array>
#include <cmath>
#include <format>
#include <utility>

#include <QAction>
#include <QMenu>
#include <QPushButton>
#include <QStatusBar>
#include <QToolButton>

#include "icons.hpp"
#include "katana/cad/plotting/arrange_commands.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/geometry/polygon.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/math/numerics.hpp"
#include "sheet_editor.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Box2;
using katana::geometry::Point2;
using plotting::AlignEdge;
using plotting::DistributeAxis;
using plotting::Sheet;
using plotting::Viewport;
using plotting::ViewportKind;

namespace {

bool isPlan(ViewportKind kind)
{
    return kind == ViewportKind::Plan || kind == ViewportKind::KeyPlan;
}

// A box's four corners, for a hull.
void addCorners(std::vector<Point2>& points, const Box2& box)
{
    if (!box.empty()) {
        points.insert(points.end(), {box.min, Point2(box.max.x, box.min.y), box.max,
                                     Point2(box.min.x, box.max.y)});
    }
}

const Sheet* currentSheetOf(const katana::cad::Document& document, const SheetEditor& editor)
{
    const auto& sheets = document.sheetSet().sheets;
    return editor.currentSheet() < sheets.size() ? &sheets[editor.currentSheet()] : nullptr;
}

const Viewport* findViewport(const katana::cad::Document& document, std::string_view id)
{
    for (const Sheet& sheet : document.sheetSet().sheets) {
        for (const Viewport& viewport : sheet.viewports) {
            if (viewport.id == id) {
                return &viewport;
            }
        }
    }
    return nullptr;
}

std::string scaleWords(double scale)
{
    return scale == std::floor(scale) ? std::format("1:{:.0f}", scale) : std::format("1:{:.2f}", scale);
}

std::string countWords(std::size_t count, std::string_view one)
{
    return std::format("{} {}{}", count, one, count == 1 ? "" : "s");
}

std::string listWords(const std::vector<std::string>& ids)
{
    std::string text;
    for (const std::string& id : ids) {
        text += (text.empty() ? "" : ", ") + id;
    }
    return text;
}

std::string_view edgeWords(AlignEdge edge)
{
    switch (edge) {
    case AlignEdge::Left: return "on their left edges";
    case AlignEdge::Right: return "on their right edges";
    case AlignEdge::Top: return "on their top edges";
    case AlignEdge::Bottom: return "on their bottom edges";
    case AlignEdge::HorizontalCentre: return "on one vertical line through their centres";
    case AlignEdge::VerticalCentre: return "on one horizontal line through their centres";
    }
    return "";
}

// The sheet must exist for a command on it.
Result<const Sheet*> sheetToArrange(const katana::cad::Document& document, const SheetEditor& editor)
{
    const Sheet* sheet = currentSheetOf(document, editor);
    if (sheet == nullptr) {
        return makeError(ErrorCode::InvalidState, "there is no sheet to arrange");
    }
    return sheet;
}

// The plan a Fit or Rotate acts on: the selected view, which must be a plan
// or key plan, or with nothing selected the sheet's main plan.
Result<const Viewport*> planToFit(const katana::cad::Document& document, const SheetEditor& editor)
{
    auto sheet = sheetToArrange(document, editor);
    if (!sheet) {
        return sheet.error();
    }
    const std::string& selected = editor.canvas()->selected();
    const std::string id =
        selected.empty() ? plotting::mainPlanOf(document.sheetSet(), editor.currentSheet()) : selected;
    const Viewport* viewport = findViewport(document, id);
    if (viewport == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "the sheet has no plan: select one, or add one");
    }
    if (!isPlan(viewport->kind)) {
        return makeError(ErrorCode::InvalidArgument,
                         "select a plan or key plan: only a view of the drawing is fitted and turned",
                         viewport->id);
    }
    return viewport;
}

// What a command did, on the status bar and in the log.
void announce(SheetEditor& editor, const Result<std::string>& outcome)
{
    const bool failed = !outcome;
    const QString text = QString::fromStdString(failed ? outcome.error().describe() : *outcome);
    editor.statusBar()->showMessage(text, 8000);
    if (editor.onMessage) {
        editor.onMessage(text, failed);
    }
}

} // namespace

std::vector<Point2> drawingContent(const SheetSource& source, const katana::cad::LayerOverrides& layers)
{
    if (source.plan.model == nullptr) {
        return {};
    }
    std::vector<Point2> points = plotting::drawnOutline(*source.plan.model, layers);
    // What planDrawnBounds counts besides the drawing: the shown imagery and
    // point clouds, and each shown mesh's footprint.
    if (source.plan.reference != nullptr) {
        for (const katana::interop::RasterOverlay& raster : source.plan.reference->rasters()) {
            if (raster.visible) {
                addCorners(points, raster.worldBounds());
            }
        }
        for (const katana::interop::PointCloudLayer& cloud : source.plan.reference->pointClouds()) {
            if (cloud.visible) {
                addCorners(points, cloud.worldBounds());
            }
        }
    }
    if (source.plan.meshes != nullptr) {
        for (const katana::cad::SceneMesh& item : *source.plan.meshes) {
            if (item.visible && item.mesh != nullptr && !item.mesh->empty() &&
                item.style != katana::cad::SurfaceStyle::Hidden) {
                const katana::math::AABB space = item.mesh->bounds();
                if (!space.empty()) {
                    addCorners(points, Box2(Point2(space.min.x, space.min.y),
                                            Point2(space.max.x, space.max.y)));
                }
            }
        }
    }
    return katana::geometry::convexHull(std::move(points));
}

std::vector<Point2> viewportContent(const Viewport& viewport, const SheetSource& source,
                                    const plotting::SheetSet& set)
{
    if (!isPlan(viewport.kind) || source.plan.model == nullptr) {
        return {};
    }
    if (viewport.kind == plotting::ViewportKind::KeyPlan) {
        // The outlines, each automatic plan where the painter puts it.
        const plotting::PlanPlacer place = [&source](const Viewport& plan) {
            const ResolvedViewport at = resolvePlanViewport(plan, source);
            return plotting::PlanPlacement{at.scale, at.centre};
        };
        std::vector<Point2> outlines = plotting::viewportContent(*source.plan.model, set, viewport, place);
        const bool none = std::ranges::none_of(set.sheets, [](const plotting::Sheet& sheet) {
            return std::ranges::any_of(sheet.viewports, [](const Viewport& v) {
                return v.kind == plotting::ViewportKind::Plan && !v.rect.empty();
            });
        });
        if (!none) {
            return outlines;
        }
    }
    std::vector<Point2> points = plotting::viewportContent(*source.plan.model, viewport);
    // A plan of the drawing also shows the window's reference layers and
    // meshes; a strip shows its stretch of the alignment only.
    const bool strip = !viewport.source.alignment.empty() &&
                       source.plan.model->alignments.find(viewport.source.alignment) != nullptr;
    if (!strip) {
        const std::vector<Point2> drawing = drawingContent(source, viewport.hiddenLayers);
        points.insert(points.end(), drawing.begin(), drawing.end());
    }
    return katana::geometry::convexHull(std::move(points));
}

double drawnScaleOf(const Viewport& viewport, const SheetSource& source, const plotting::SheetSet& set)
{
    if (isPlan(viewport.kind) && viewport.autoScale) {
        // With the set and its sheet, so an automatic key plan is at the
        // scale it is drawn at: fitted to its outlines, not to the drawing.
        std::size_t onSheet = set.sheets.size();
        for (std::size_t i = 0; i < set.sheets.size(); ++i) {
            if (std::ranges::any_of(set.sheets[i].viewports,
                                    [&viewport](const Viewport& v) { return v.id == viewport.id; })) {
                onSheet = i;
                break;
            }
        }
        return onSheet < set.sheets.size()
                   ? resolvePlanViewport(viewport, source, set, onSheet).scale
                   : resolvePlanViewport(viewport, source).scale;
    }
    return viewport.scale;
}

std::vector<std::string> arrangeTargets(const katana::cad::Document& document, const SheetEditor& editor)
{
    const Sheet* sheet = currentSheetOf(document, editor);
    if (sheet == nullptr) {
        return {};
    }
    if (const std::string& selected = editor.canvas()->selected(); !selected.empty()) {
        return {selected};
    }
    std::vector<std::string> ids;
    for (const Viewport& viewport : sheet->viewports) {
        if (!viewport.rect.empty()) {
            ids.push_back(viewport.id);
        }
    }
    return ids;
}

// ---- The commands -----------------------------------------------------------------------

Result<std::string> arrangeSheet(katana::cad::Document& document, SheetEditor& editor)
{
    if (auto sheet = sheetToArrange(document, editor); !sheet) {
        return sheet.error();
    }
    const auto result = plotting::autoArrangeSheet(document, editor.currentSheet());
    if (!result) {
        return result.error();
    }
    std::string text = result->moved.empty()
                           ? std::string("Nothing overlaps: the sheet is arranged already.")
                           : std::format("Arranged the sheet: moved {}.",
                                         countWords(result->moved.size(), "view"));
    if (result->mainShrunk) {
        text += " The main view was made smaller to make room.";
    }
    if (!result->overlapping.empty()) {
        text += std::format(" No room for {}: still overlapping.", listWords(result->overlapping));
    }
    if (!result->unplaced.empty()) {
        text += std::format(" No room for {}: not placed.", listWords(result->unplaced));
    }
    return text;
}

Result<std::string> alignSelection(katana::cad::Document& document, SheetEditor& editor, AlignEdge edge)
{
    if (auto sheet = sheetToArrange(document, editor); !sheet) {
        return sheet.error();
    }
    const std::vector<std::string> ids = arrangeTargets(document, editor);
    const auto moved = plotting::alignViewports(document, editor.currentSheet(), ids, edge);
    if (!moved) {
        return moved.error();
    }
    if (moved->empty()) {
        return std::string("Nothing moved: the views are aligned already.");
    }
    return std::format("Aligned {} {}.", countWords(moved->size(), "view"), edgeWords(edge));
}

Result<std::string> distributeSelection(katana::cad::Document& document, SheetEditor& editor,
                                        DistributeAxis axis)
{
    auto sheet = sheetToArrange(document, editor);
    if (!sheet) {
        return sheet.error();
    }
    std::vector<std::string> ids = arrangeTargets(document, editor);
    if (ids.size() < 3) {
        ids.clear();
        for (const Viewport& viewport : (*sheet)->viewports) {
            if (!viewport.rect.empty()) {
                ids.push_back(viewport.id);
            }
        }
    }
    const auto moved = plotting::distributeViewports(document, editor.currentSheet(), ids, axis);
    if (!moved) {
        return moved.error();
    }
    if (moved->empty()) {
        return std::string("Nothing moved: the views are evenly spaced already.");
    }
    return std::format("Spaced {} evenly {} the sheet.", countWords(ids.size(), "view"),
                       axis == DistributeAxis::Horizontal ? "across" : "up");
}

Result<std::string> matchSelectionScale(katana::cad::Document& document, SheetEditor& editor,
                                        const std::string& fromId)
{
    if (auto sheet = sheetToArrange(document, editor); !sheet) {
        return sheet.error();
    }
    const Viewport* from = findViewport(document, fromId);
    if (from == nullptr) {
        return makeError(ErrorCode::NotFound, "no viewport of that id to take the scale from", fromId);
    }
    const double scale = drawnScaleOf(*from, editor.source(), document.sheetSet());
    const std::vector<std::string> ids = arrangeTargets(document, editor);
    const auto changed = plotting::matchScale(document, ids, fromId, scale);
    if (!changed) {
        return changed.error();
    }
    if (changed->empty()) {
        return std::format("Nothing changed: the views are at {} already, or have no scale.",
                           scaleWords(scale));
    }
    return std::format("Gave {} the scale of {}, {}.", listWords(*changed), fromId, scaleWords(scale));
}

Result<std::string> fitSelectionToContent(katana::cad::Document& document, SheetEditor& editor)
{
    auto plan = planToFit(document, editor);
    if (!plan) {
        return plan.error();
    }
    const Viewport viewport = **plan;
    const SheetSource source = editor.source();
    const std::vector<Point2> content = viewportContent(viewport, source, document.sheetSet());
    if (content.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the view shows nothing to fit to", viewport.id);
    }
    const double scale = drawnScaleOf(viewport, source, document.sheetSet());
    if (auto status = plotting::fitViewportToContent(document, viewport.id, content, scale); !status) {
        return status.error();
    }
    const Viewport* fitted = findViewport(document, viewport.id);
    return std::format("Sized {} to what it shows: {:.1f} x {:.1f} mm at {}.", viewport.id,
                       fitted->rect.width(), fitted->rect.height(), scaleWords(scale));
}

Result<std::string> rotateSelectionToBestFit(katana::cad::Document& document, SheetEditor& editor)
{
    auto plan = planToFit(document, editor);
    if (!plan) {
        return plan.error();
    }
    const std::string id = (*plan)->id;
    const std::vector<Point2> content =
        viewportContent(**plan, editor.source(), document.sheetSet());
    if (content.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the view shows nothing to fit to", id);
    }
    const auto fit = plotting::rotateToBestFit(document, id, content);
    if (!fit) {
        return fit.error();
    }
    if (fit->rotation == 0.0) {
        return std::format("{} shows most square to the paper: north up at {}.", id,
                           scaleWords(fit->standardScale));
    }
    return std::format("Turned {} to {:.2f} degrees, at {}.", id,
                       fit->rotation * katana::math::kRadToDeg, scaleWords(fit->standardScale));
}

Result<std::string> choosePaperForSheet(katana::cad::Document& document, SheetEditor& editor)
{
    if (auto sheet = sheetToArrange(document, editor); !sheet) {
        return sheet.error();
    }
    const std::string id = plotting::mainPlanOf(document.sheetSet(), editor.currentSheet());
    const Viewport* plan = findViewport(document, id);
    if (plan == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "the sheet has no plan to choose paper for");
    }
    const SheetSource source = editor.source();
    const std::vector<Point2> content = viewportContent(*plan, source, document.sheetSet());
    if (content.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the plan shows nothing to choose paper for", id);
    }
    const double scale = drawnScaleOf(*plan, source, document.sheetSet());
    const auto change = plotting::choosePaperForScale(document, id, content, scale);
    if (!change) {
        return change.error();
    }
    return std::format("A{} {} holds {} at {}.", static_cast<int>(change->advice.paper),
                       change->advice.landscape ? "landscape" : "portrait", id, scaleWords(scale));
}

// ---- The menus --------------------------------------------------------------------------

void fillArrangeMenu(QMenu& menu, katana::cad::Document& document, SheetEditor& editor)
{
    using Command = Result<std::string> (*)(katana::cad::Document&, SheetEditor&);
    const auto add = [&menu, &document, &editor](const QString& text, const char* name,
                                                 const QString& tip, auto&& run) {
        QAction* action = menu.addAction(text);
        action->setObjectName(QString::fromLatin1(name));
        action->setToolTip(tip);
        action->setStatusTip(tip);
        QObject::connect(action, &QAction::triggered, &editor,
                         [&document, &editor, run] { announce(editor, run(document, editor)); });
        return action;
    };
    add(QStringLiteral("Auto Arrange"), "sheetArrangeAuto",
        QStringLiteral("Move the views apart so none overlaps, the main view keeping its place"),
        static_cast<Command>(&arrangeSheet));
    menu.addSeparator();

    struct Align {
        AlignEdge edge;
        const char* text;
        const char* name;
    };
    constexpr std::array kAligns{
        Align{AlignEdge::Left, "Align Left", "sheetAlignLeft"},
        Align{AlignEdge::Right, "Align Right", "sheetAlignRight"},
        Align{AlignEdge::Top, "Align Top", "sheetAlignTop"},
        Align{AlignEdge::Bottom, "Align Bottom", "sheetAlignBottom"},
        Align{AlignEdge::HorizontalCentre, "Align Centres Horizontally", "sheetAlignHCentre"},
        Align{AlignEdge::VerticalCentre, "Align Centres Vertically", "sheetAlignVCentre"},
    };
    for (const Align& align : kAligns) {
        const AlignEdge edge = align.edge;
        add(QString::fromLatin1(align.text), align.name,
            QStringLiteral("The selected view to the tiling area, or with nothing selected every view "
                           "to the others"),
            [edge](katana::cad::Document& d, SheetEditor& e) { return alignSelection(d, e, edge); });
    }
    menu.addSeparator();
    add(QStringLiteral("Distribute Horizontally"), "sheetDistributeHorizontal",
        QStringLiteral("Equal gaps across the sheet between the views"),
        [](katana::cad::Document& d, SheetEditor& e) {
            return distributeSelection(d, e, DistributeAxis::Horizontal);
        });
    add(QStringLiteral("Distribute Vertically"), "sheetDistributeVertical",
        QStringLiteral("Equal gaps up the sheet between the views"),
        [](katana::cad::Document& d, SheetEditor& e) {
            return distributeSelection(d, e, DistributeAxis::Vertical);
        });
    menu.addSeparator();

    QMenu* match = menu.addMenu(QStringLiteral("Match Scale To"));
    match->setObjectName(QStringLiteral("sheetMatchScaleMenu"));
    match->setToolTip(QStringLiteral("Give the selected view - or every view on the sheet - another view's scale"));
    QObject::connect(match, &QMenu::aboutToShow, &editor,
                     [match, &document, &editor] { fillMatchScaleMenu(*match, document, editor); });
    add(QStringLiteral("Fit View to Content"), "sheetFitToContent",
        QStringLiteral("Size the selected plan (or the main plan) so what it shows just fits at its scale"),
        static_cast<Command>(&fitSelectionToContent));
    add(QStringLiteral("Rotate to Best Fit"), "sheetRotateToBestFit",
        QStringLiteral("Turn the selected plan (or the main plan) as far as it takes to show the drawing "
                       "at the largest scale"),
        static_cast<Command>(&rotateSelectionToBestFit));
}

void fillMatchScaleMenu(QMenu& menu, katana::cad::Document& document, SheetEditor& editor)
{
    menu.clear();
    const plotting::SheetSet& set = document.sheetSet();
    const std::string& selected = editor.canvas()->selected();
    const SheetSource source = editor.source();
    for (std::size_t s = 0; s < set.sheets.size(); ++s) {
        const Sheet& sheet = set.sheets[s];
        for (const Viewport& viewport : sheet.viewports) {
            if (viewport.id == selected || !plotting::hasScale(viewport.kind)) {
                continue;
            }
            const std::string title = viewport.title.empty()
                                          ? std::string(plotting::toString(viewport.kind))
                                          : viewport.title;
            const std::string text =
                std::format("{}  {}  ({}, {})", viewport.id, title,
                            sheet.name.empty() ? std::format("sheet {}", s + 1) : sheet.name,
                            scaleWords(drawnScaleOf(viewport, source, document.sheetSet())));
            QAction* action = menu.addAction(QString::fromStdString(text));
            action->setObjectName(QString::fromStdString("sheetMatchScale_" + viewport.id));
            const std::string id = viewport.id;
            QObject::connect(action, &QAction::triggered, &editor, [&document, &editor, id] {
                announce(editor, matchSelectionScale(document, editor, id));
            });
        }
    }
    if (menu.isEmpty()) {
        menu.addAction(QStringLiteral("(no other view is drawn to a scale)"))->setEnabled(false);
    }
}

QToolButton* arrangeToolButton(QWidget* parent, katana::cad::Document& document, SheetEditor& editor)
{
    auto* button = new QToolButton(parent);
    button->setObjectName(QStringLiteral("sheetArrangeButton"));
    button->setText(QStringLiteral("Arrange"));
    button->setIcon(icon(Icon::Move));
    button->setToolTip(QStringLiteral("Arrange, align, space, match, fit and turn the views"));
    button->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    button->setPopupMode(QToolButton::InstantPopup);
    auto* menu = new QMenu(button);
    menu->setObjectName(QStringLiteral("sheetArrangeMenu"));
    fillArrangeMenu(*menu, document, editor);
    button->setMenu(menu);
    return button;
}

QPushButton* choosePaperButton(QWidget* parent, katana::cad::Document& document, SheetEditor& editor)
{
    auto* button = new QPushButton(QStringLiteral("Choose paper for this scale"), parent);
    button->setObjectName(QStringLiteral("sheetChoosePaper"));
    const std::string id = plotting::mainPlanOf(document.sheetSet(), editor.currentSheet());
    button->setEnabled(!id.empty());
    button->setToolTip(
        id.empty() ? QStringLiteral("The sheet has no plan to choose paper for")
                   : QString("The smallest paper that holds %1 at the scale it is drawn at, the views "
                             "kept in proportion")
                         .arg(QString::fromStdString(id)));
    QObject::connect(button, &QPushButton::clicked, &editor,
                     [&document, &editor] { announce(editor, choosePaperForSheet(document, editor)); });
    return button;
}

} // namespace katana::qt
