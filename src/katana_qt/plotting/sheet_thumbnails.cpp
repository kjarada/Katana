#include "plotting/sheet_thumbnails.hpp"

#include <algorithm>
#include <cmath>
#include <set>

#include <QPainter>

#include "katana/cad/plot.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using plotting::Sheet;
using plotting::SheetSet;
using plotting::ViewportKind;

namespace {

// The project's contribution to the title block, as one string to compare.
std::string fieldsText(const plotting::FieldContext& fields)
{
    std::string text;
    for (const std::string* part : {&fields.projectName, &fields.projectDescription,
                                    &fields.coordinateSystem, &fields.fileName, &fields.plotDate}) {
        text += *part;
        text += '\x1f';
    }
    return text;
}

bool sameOrder(const std::vector<std::string>& order, const SheetSet& set)
{
    return std::ranges::equal(order, set.sheets, {}, {}, &Sheet::id);
}

} // namespace

bool showsTheDrawing(const Sheet& sheet)
{
    return std::ranges::any_of(sheet.viewports, [](const plotting::Viewport& viewport) {
        switch (viewport.kind) {
        case ViewportKind::Notes:
        case ViewportKind::Image:
            return false;
        case ViewportKind::Plan:
        case ViewportKind::LongSection:
        case ViewportKind::CrossSections:
        case ViewportKind::Model3D:
        case ViewportKind::Legend:
        case ViewportKind::KeyPlan:
            return true;
        }
        return true;
    });
}

SheetThumbnails::SheetThumbnails(QSize box) : box_(box) {}

bool SheetThumbnails::matches(const Key& key, const SheetSet& set, std::size_t index,
                              const SheetSource& source)
{
    const Sheet& sheet = set.sheets[index];
    return key.index == index && key.sheet == sheet && sameOrder(key.order, set) &&
           key.defaults == set.defaults && key.numbering == set.numbering &&
           key.revisions == set.revisions && key.fields == fieldsText(source.fields) &&
           (!showsTheDrawing(sheet) || key.revision == source.revision);
}

bool SheetThumbnails::isStale(const SheetSet& set, std::size_t index, const SheetSource& source) const
{
    if (index >= set.sheets.size()) {
        return false;
    }
    const auto it = entries_.find(set.sheets[index].id);
    return it == entries_.end() || !matches(it->second.key, set, index, source);
}

std::size_t SheetThumbnails::staleCount(const SheetSet& set, const SheetSource& source) const
{
    std::size_t stale = 0;
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        stale += isStale(set, i, source) ? 1 : 0;
    }
    return stale;
}

QImage SheetThumbnails::thumbnail(const SheetSet& set, std::size_t index, const SheetSource& source,
                                  double pixelRatio)
{
    if (index >= set.sheets.size()) {
        return {};
    }
    const Sheet& sheet = set.sheets[index];
    if (auto it = entries_.find(sheet.id);
        it != entries_.end() && it->second.key.pixelRatio == pixelRatio &&
        matches(it->second.key, set, index, source)) {
        return it->second.image;
    }
    const double ratio = pixelRatio > 0.0 ? pixelRatio : 1.0;
    const auto paper = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    // The paper's shape inside the box, a pixel clear of its edge for the
    // outline.
    const double logical = std::min((box_.width() - 2.0) / paper.widthMm,
                                    (box_.height() - 2.0) / paper.heightMm);
    const double ppmm = logical * ratio;
    QImage image(static_cast<int>(std::lround(box_.width() * ratio)),
                 static_cast<int>(std::lround(box_.height() * ratio)),
                 QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::transparent);
    SheetPaintOptions options;
    options.pixelsPerMillimetre = ppmm;
    options.origin = QPointF((image.width() - paper.widthMm * ppmm) / 2.0,
                             (image.height() - paper.heightMm * ppmm) / 2.0);
    // A thumbnail's 3D snapshot is a few dozen pixels: never render more.
    options.rasterDpiCap = 25.0;
    options.rasterPixelCap = 60'000;
    {
        QPainter painter(&image);
        painter.setRenderHint(QPainter::Antialiasing, true);
        (void)paintSheet(painter, set, index, source, options, cache_);
        painter.setPen(QPen(QColor(120, 122, 128), 1.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawRect(QRectF(options.origin, QSizeF(paper.widthMm * ppmm, paper.heightMm * ppmm))
                             .adjusted(0.5, 0.5, -0.5, -0.5));
    }
    image.setDevicePixelRatio(ratio);
    ++renders_;

    Key key;
    key.sheet = sheet;
    key.index = index;
    for (const Sheet& each : set.sheets) {
        key.order.push_back(each.id);
    }
    key.defaults = set.defaults;
    key.numbering = set.numbering;
    key.revisions = set.revisions;
    key.fields = fieldsText(source.fields);
    key.revision = source.revision;
    key.pixelRatio = pixelRatio;
    entries_.insert_or_assign(sheet.id, Entry{std::move(key), image});
    return image;
}

QImage SheetThumbnails::cached(std::string_view sheetId) const
{
    const auto it = entries_.find(sheetId);
    return it != entries_.end() ? it->second.image : QImage();
}

void SheetThumbnails::prune(const SheetSet& set)
{
    std::set<std::string, std::less<>> ids;
    for (const Sheet& sheet : set.sheets) {
        ids.insert(sheet.id);
    }
    std::erase_if(entries_, [&ids](const auto& entry) { return !ids.contains(entry.first); });
}

void SheetThumbnails::clear()
{
    entries_.clear();
    cache_.clear();
}

} // namespace katana::qt
