#include "katana/cad/plotting/layout.hpp"

#include <algorithm>
#include <array>
#include <cmath>

namespace katana::cad::plotting {

namespace {

// A cell as the owner's app defines it: [x, y, w, h] as fractions of the
// area, measured from its TOP-left - the app draws in SVG, Y down. cellBox
// turns that into paper coordinates, Y up.
struct Fraction {
    double x;
    double y;
    double w;
    double h;
};

struct PresetDefinition {
    TilingPreset preset;
    std::string_view id;
    std::string_view name;
    std::vector<Fraction> cells;
};

const std::array<PresetDefinition, kTilingPresetCount>& presets()
{
    static const std::array<PresetDefinition, kTilingPresetCount> table{{
        {TilingPreset::Full, "full", "Full frame", {{0, 0, 1, 1}}},
        {TilingPreset::Columns2, "cols2", "Two columns", {{0, 0, 0.5, 1}, {0.5, 0, 0.5, 1}}},
        {TilingPreset::Rows2, "rows2", "Two rows", {{0, 0, 1, 0.5}, {0, 0.5, 1, 0.5}}},
        {TilingPreset::Quad,
         "quad",
         "Quarters",
         {{0, 0, 0.5, 0.5}, {0.5, 0, 0.5, 0.5}, {0, 0.5, 0.5, 0.5}, {0.5, 0.5, 0.5, 0.5}}},
        {TilingPreset::MainRight,
         "sectionR",
         "Main and panel right",
         {{0, 0, 0.66, 1}, {0.66, 0, 0.34, 1}}},
        {TilingPreset::MainBelow,
         "sectionB",
         "Main and panel below",
         {{0, 0, 1, 0.64}, {0, 0.64, 1, 0.36}}},
        {TilingPreset::MainCorner,
         "sectionBR",
         "Main and panel in the corner",
         {{0, 0, 1, 0.66}, {0.62, 0.66, 0.38, 0.34}}},
        {TilingPreset::MainTwoRight,
         "sectionMap3d",
         "Main and two panels right",
         {{0, 0, 0.64, 1}, {0.64, 0, 0.36, 0.5}, {0.64, 0.5, 0.36, 0.5}}},
    }};
    return table;
}

const PresetDefinition& definitionOf(TilingPreset preset)
{
    for (const PresetDefinition& definition : presets()) {
        if (definition.preset == preset) {
            return definition;
        }
    }
    return presets().front();
}

// The cell in paper coordinates: its share of the area, less half the gutter
// on every side, at least `minimum` in size, growing right and DOWN from its
// top-left corner as the app's cells do.
Box2 cellBox(const Fraction& cell, const Box2& area, double gutter, SizeMm minimum)
{
    const double left = area.min.x + cell.x * area.width() + gutter / 2.0;
    const double top = area.max.y - cell.y * area.height() - gutter / 2.0;
    const double width = std::max(minimum.width, cell.w * area.width() - gutter);
    const double height = std::max(minimum.height, cell.h * area.height() - gutter);
    return Box2(Point2(left, top - height), Point2(left + width, top));
}

// The smallest shift within `tolerance` that puts one of `mine` on one of
// `targets`, and the target it lands on. Ties keep the first found, so the
// answer does not depend on anything but the order of the inputs.
std::optional<std::pair<double, double>> bestShift(const std::array<double, 3>& mine,
                                                   const std::vector<double>& targets,
                                                   double tolerance)
{
    std::optional<std::pair<double, double>> best;
    for (const double own : mine) {
        for (const double target : targets) {
            const double shift = target - own;
            if (std::abs(shift) <= tolerance &&
                (!best || std::abs(shift) < std::abs(best->first))) {
                best = std::pair{shift, target};
            }
        }
    }
    return best;
}

} // namespace

std::string_view presetId(TilingPreset preset)
{
    return definitionOf(preset).id;
}

std::optional<TilingPreset> presetFromId(std::string_view id)
{
    for (const PresetDefinition& definition : presets()) {
        if (definition.id == id) {
            return definition.preset;
        }
    }
    return std::nullopt;
}

std::string_view presetName(TilingPreset preset)
{
    return definitionOf(preset).name;
}

int tilingRank(ViewportKind kind)
{
    switch (kind) {
    case ViewportKind::Plan:
        return 0;
    case ViewportKind::LongSection:
        return 1;
    case ViewportKind::CrossSections:
        return 2;
    case ViewportKind::Model3D:
        return 3;
    case ViewportKind::KeyPlan:
        return 4;
    case ViewportKind::Image:
        return 5;
    case ViewportKind::Legend:
        return 6;
    case ViewportKind::Notes:
        return 7;
    // A register sheet's tables: the register takes the big cell, the
    // revisions the panel beside it (registerSheet, tables.hpp).
    case ViewportKind::SheetIndex:
        return 8;
    case ViewportKind::Revisions:
        return 9;
    }
    return 10;
}

bool isTileable(ViewportKind kind)
{
    return kind != ViewportKind::Notes;
}

SizeMm minimumSize(ViewportKind kind)
{
    // The owner's app's minimum panel sizes, by the panel each kind was
    // (section, map, locality, 3D view, image, note).
    switch (kind) {
    case ViewportKind::LongSection:
    case ViewportKind::CrossSections:
        return {70.0, 45.0};
    case ViewportKind::Plan:
        return {35.0, 30.0};
    case ViewportKind::KeyPlan:
        return {30.0, 26.0};
    case ViewportKind::Model3D:
        return {30.0, 24.0};
    case ViewportKind::Image:
        return {8.0, 8.0};
    case ViewportKind::Legend:
    case ViewportKind::Notes:
        return {16.0, 8.0};
    // Kinds the app did not have: room for the columns and a few rows of
    // the smallest text a table is set in (tables.hpp).
    case ViewportKind::SheetIndex:
        return {70.0, 30.0};
    case ViewportKind::Revisions:
        return {50.0, 20.0};
    }
    return {8.0, 8.0};
}

std::vector<Box2> presetCells(TilingPreset preset, const Box2& area, double gutterMm)
{
    std::vector<Box2> cells;
    for (const Fraction& cell : definitionOf(preset).cells) {
        cells.push_back(cellBox(cell, area, gutterMm, {}));
    }
    return cells;
}

Box2 tilingArea(const Sheet& sheet)
{
    return drawingArea(sheet).inflated(-kTilingInsetMm);
}

std::size_t tileViewports(Sheet& sheet, TilingPreset preset, double gutterMm)
{
    std::vector<Viewport*> order;
    for (Viewport& viewport : sheet.viewports) {
        if (!viewport.locked && isTileable(viewport.kind)) {
            order.push_back(&viewport);
        }
    }
    std::stable_sort(order.begin(), order.end(), [](const Viewport* a, const Viewport* b) {
        return tilingRank(a->kind) < tilingRank(b->kind);
    });
    const Box2 area = tilingArea(sheet);
    const std::vector<Fraction>& cells = definitionOf(preset).cells;
    const std::size_t placed = std::min(order.size(), cells.size());
    for (std::size_t i = 0; i < placed; ++i) {
        order[i]->rect = cellBox(cells[i], area, gutterMm, minimumSize(order[i]->kind));
    }
    return placed;
}

SnapResult snapRect(const Box2& moving, const Box2& drawingArea, std::span<const Box2> others,
                    double toleranceMm)
{
    SnapResult result{moving, std::nullopt, std::nullopt};
    if (!(toleranceMm > 0.0) || moving.empty()) {
        return result;
    }
    std::vector<double> xs;
    std::vector<double> ys;
    const auto addTargets = [&xs, &ys](const Box2& box) {
        if (box.empty()) {
            return;
        }
        xs.insert(xs.end(), {box.min.x, box.max.x, box.center().x});
        ys.insert(ys.end(), {box.min.y, box.max.y, box.center().y});
    };
    addTargets(drawingArea);
    for (const Box2& other : others) {
        addTargets(other);
    }
    const std::array<double, 3> ownX{moving.min.x, moving.max.x, moving.center().x};
    const std::array<double, 3> ownY{moving.min.y, moving.max.y, moving.center().y};
    if (const auto shift = bestShift(ownX, xs, toleranceMm)) {
        result.rect.min.x += shift->first;
        result.rect.max.x += shift->first;
        result.guideX = shift->second;
    }
    if (const auto shift = bestShift(ownY, ys, toleranceMm)) {
        result.rect.min.y += shift->first;
        result.rect.max.y += shift->first;
        result.guideY = shift->second;
    }
    return result;
}

} // namespace katana::cad::plotting
