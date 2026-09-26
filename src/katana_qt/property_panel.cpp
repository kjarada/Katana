#include "property_panel.hpp"

#include <QAbstractItemModel>
#include <QAction>
#include <QApplication>
#include <QBrush>
#include <QClipboard>
#include <QFont>
#include <QHeaderView>
#include <QKeySequence>
#include <QLineEdit>
#include <QScrollBar>
#include <QSet>
#include <QTimer>
#include <QTreeView>
#include <QVBoxLayout>

#include <algorithm>
#include <map>
#include <optional>
#include <string>
#include <utility>
#include <variant>

#include "format.hpp"
#include "icons.hpp"
#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/property_outline.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/math/numerics.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::PropertyMap;
using katana::geometry::Point2;

constexpr int kKeyRole = Qt::UserRole + 1;  // a row's key among its siblings
constexpr int kMoreRole = Qt::UserRole + 2; // true on a Show more row

// The longest value drawn in the Value column, and the longest a tooltip
// carries: a per-vertex list of heights is thousands of characters, and a
// row that long, or a tooltip, is a wall nobody reads. The whole value is
// PROP LIST's and Edit > Attributes'.
constexpr qsizetype kShownCharacters = 160;
constexpr qsizetype kTipCharacters = 1200;

// Opened groups are remembered by their key path, the keys joined by a
// character no key holds.
constexpr QChar kPathSeparator(0x1f);

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

QString shown(const QString& text)
{
    return text.size() <= kShownCharacters ? text
                                           : text.left(kShownCharacters) + QStringLiteral("…");
}

// Rich text, so a value such as "<varies>" is not read as a tag.
QString tipFor(const QString& name, const QString& text)
{
    const QString body =
        text.size() <= kTipCharacters
            ? text
            : text.left(kTipCharacters) + QString("… (%1 characters)").arg(grouped(text.size()));
    return QString("<b>%1</b><br>%2").arg(name.toHtmlEscaped(), body.toHtmlEscaped());
}

QString countOf(std::size_t count, const char* one, const char* many)
{
    return grouped(count) + " " + (count == 1 ? one : many);
}

PropertyRow leaf(const QString& label, const QString& value)
{
    PropertyRow row;
    row.label = label;
    row.value = shown(value);
    row.key = label;
    if (value.size() > kShownCharacters) {
        row.tip = tipFor(label, value);
    }
    return row;
}

PropertyRow heading(const QString& label, const QString& key, const QString& summary = {})
{
    PropertyRow row;
    row.label = label;
    row.key = key;
    row.value = summary;
    row.heading = true;
    row.muted = true;
    return row;
}

// ---- the model --------------------------------------------------------------------------

// A tree whose levels are read when opened and made a page at a time. No
// Q_OBJECT: it declares no signals of its own.
class PropertyTreeModel final : public QAbstractItemModel {
  public:
    struct Node {
        Node* parent = nullptr;
        int row = 0;
        PropertyRow data;
        bool isMore = false;
        std::optional<PropertyLevel> level;
        std::vector<std::unique_ptr<Node>> children;
        // The Show more row, while the level has rows not yet made.
        std::unique_ptr<Node> more;
    };

    using QAbstractItemModel::QAbstractItemModel;

    void reset(PropertyLevel top)
    {
        beginResetModel();
        root_ = std::make_unique<Node>();
        root_->level = std::move(top);
        makePage(*root_, QModelIndex(), false);
        endResetModel();
    }

    [[nodiscard]] Node* nodeOf(const QModelIndex& index) const
    {
        return index.isValid() ? static_cast<Node*>(index.internalPointer()) : root_.get();
    }

    [[nodiscard]] QModelIndex index(int row, int column, const QModelIndex& parent) const override
    {
        const Node* owner = nodeOf(parent);
        if (owner == nullptr || owner->isMore || row < 0 || column < 0 || column > 1) {
            return {};
        }
        const auto made = static_cast<int>(owner->children.size());
        if (row < made) {
            return createIndex(row, column, owner->children[static_cast<std::size_t>(row)].get());
        }
        if (row == made && owner->more) {
            return createIndex(row, column, owner->more.get());
        }
        return {};
    }

    [[nodiscard]] QModelIndex parent(const QModelIndex& child) const override
    {
        if (!child.isValid()) {
            return {};
        }
        const Node* owner = static_cast<const Node*>(child.internalPointer())->parent;
        if (owner == nullptr || owner == root_.get()) {
            return {};
        }
        return createIndex(owner->row, 0, owner);
    }

    [[nodiscard]] int rowCount(const QModelIndex& parent) const override
    {
        if (parent.column() > 0) {
            return 0;
        }
        const Node* owner = nodeOf(parent);
        if (owner == nullptr || owner->isMore) {
            return 0;
        }
        return static_cast<int>(owner->children.size()) + (owner->more ? 1 : 0);
    }

    [[nodiscard]] int columnCount(const QModelIndex& /*parent*/) const override { return 2; }

    // An expander before the level is read: that is what lets a row with
    // thirty thousand values beneath it cost nothing until it is opened.
    [[nodiscard]] bool hasChildren(const QModelIndex& parent) const override
    {
        const Node* owner = nodeOf(parent);
        if (owner == nullptr || owner->isMore) {
            return false;
        }
        if (owner == root_.get()) {
            return rowCount(parent) > 0;
        }
        if (owner->level) {
            return owner->level->count > 0;
        }
        return static_cast<bool>(owner->data.children);
    }

    // Only a level not yet read. QTreeView fetches from inside its own
    // layout of the row it is opening, and its row removal clears the items
    // that layout is filling - so a fetch that took the Show more row away
    // there wrote past the end of the view's list (found by valgrind in
    // ALongStringIsMadeAPageAtATimeWithAShowMoreRow). The first page adds
    // rows and removes none; the pages after it are made by showMore, from
    // the event loop.
    [[nodiscard]] bool canFetchMore(const QModelIndex& parent) const override
    {
        const Node* owner = nodeOf(parent);
        return owner != nullptr && !owner->isMore && !owner->level &&
               static_cast<bool>(owner->data.children);
    }

    // Reads the level and makes its first page.
    void fetchMore(const QModelIndex& parent) override
    {
        if (!canFetchMore(parent)) {
            return;
        }
        Node& owner = *nodeOf(parent);
        owner.level = owner.data.children();
        makePage(owner, parent, true);
    }

    // The next page of a level already read, where its Show more row was.
    // Never from inside a view's layout (see canFetchMore).
    bool showMore(const QModelIndex& parent)
    {
        Node* owner = nodeOf(parent);
        if (owner == nullptr || owner->isMore || !owner->level ||
            owner->children.size() >= owner->level->count) {
            return false;
        }
        makePage(*owner, parent, true);
        return true;
    }

    [[nodiscard]] QVariant data(const QModelIndex& index, int role) const override
    {
        if (!index.isValid()) {
            return {};
        }
        const Node& node = *static_cast<const Node*>(index.internalPointer());
        const PropertyRow& row = node.data;
        switch (role) {
        case Qt::DisplayRole:
            return index.column() == 0 ? row.label : row.value;
        case Qt::ToolTipRole:
            return row.tip.isEmpty() ? QVariant() : QVariant(row.tip);
        case Qt::FontRole:
            // A group's name bold, its count beside it plain.
            if ((row.heading && index.column() == 0) || node.isMore) {
                QFont font = QApplication::font();
                font.setBold(row.heading);
                font.setItalic(node.isMore);
                return font;
            }
            return {};
        case Qt::ForegroundRole:
            if (node.isMore) {
                return QBrush(theme::accent());
            }
            if (row.muted && index.column() == 1) {
                return QBrush(theme::textMuted());
            }
            return {};
        case kKeyRole:
            return row.key;
        case kMoreRole:
            return node.isMore;
        default:
            return {};
        }
    }

    [[nodiscard]] QVariant headerData(int section, Qt::Orientation orientation,
                                      int role) const override
    {
        if (orientation != Qt::Horizontal || role != Qt::DisplayRole) {
            return {};
        }
        return section == 0 ? QStringLiteral("Property") : QStringLiteral("Value");
    }

    [[nodiscard]] Qt::ItemFlags flags(const QModelIndex& index) const override
    {
        return index.isValid() ? Qt::ItemIsEnabled | Qt::ItemIsSelectable : Qt::NoItemFlags;
    }

  private:
    // The next page of `owner`'s level, with a Show more row after it while
    // rows are left. `notify` is false inside a reset, which tells the views
    // everything at once.
    void makePage(Node& owner, const QModelIndex& parent, bool notify)
    {
        const std::size_t made = owner.children.size();
        const std::size_t total = owner.level ? owner.level->count : 0;
        const std::size_t page = std::min(kPropertyPage, total - std::min(total, made));
        if (owner.more) {
            if (notify) {
                beginRemoveRows(parent, static_cast<int>(made), static_cast<int>(made));
            }
            owner.more.reset();
            if (notify) {
                endRemoveRows();
            }
        }
        if (page > 0) {
            if (notify) {
                beginInsertRows(parent, static_cast<int>(made), static_cast<int>(made + page - 1));
            }
            for (std::size_t i = made; i < made + page; ++i) {
                auto node = std::make_unique<Node>();
                node->parent = &owner;
                node->row = static_cast<int>(i);
                node->data = owner.level->row(i);
                owner.children.push_back(std::move(node));
            }
            if (notify) {
                endInsertRows();
            }
        }
        if (owner.children.size() < total) {
            const std::size_t now = owner.children.size();
            if (notify) {
                beginInsertRows(parent, static_cast<int>(now), static_cast<int>(now));
            }
            owner.more = std::make_unique<Node>();
            owner.more->parent = &owner;
            owner.more->row = static_cast<int>(now);
            owner.more->isMore = true;
            owner.more->data.label =
                QString("Show %1 more").arg(grouped(std::min(kPropertyPage, total - now)));
            owner.more->data.value = QString("%1 of %2 shown").arg(grouped(now), grouped(total));
            owner.more->data.tip = QStringLiteral(
                "<b>Show more</b><br>Click to make the next rows. A level is read a page at a "
                "time, so that a string with thousands of vertices opens at once.");
            owner.more->data.key = QStringLiteral("\x1fmore");
            if (notify) {
                endInsertRows();
            }
        }
    }

    std::unique_ptr<Node> root_ = std::make_unique<Node>();
};

// ---- what the rows say ------------------------------------------------------------------

// Where a level's properties come from: the entities, looked up afresh each
// time a level is read, so a level opened after an edit reads the drawing as
// it is (and one whose entity has gone reads nothing).
struct Subject {
    const katana::cad::Document* document = nullptr;
    std::vector<EntityId> ids;
    // The entities' source records (Entity::metadata) rather than their
    // properties.
    bool metadata = false;
    // One entity whose vertices are listed in a Vertices group, which then
    // holds their attributes and heights: how many.
    std::size_t verticesShown = 0;

    [[nodiscard]] std::vector<const PropertyMap*> maps() const
    {
        std::vector<const PropertyMap*> out;
        out.reserve(ids.size());
        for (const EntityId id : ids) {
            if (const Entity* entity = document->model().entities.find(id)) {
                out.push_back(metadata ? &entity->metadata : &entity->properties);
            }
        }
        return out;
    }
};

using SubjectPtr = std::shared_ptr<const Subject>;

// A vertex's and a segment's number read as one: "Vertex 3".
QString entryLabel(std::string_view parent, std::string_view name)
{
    const bool numbered =
        !name.empty() && std::ranges::all_of(name, [](char c) { return c >= '0' && c <= '9'; });
    if (numbered && (parent == "vertex" || parent == "segment")) {
        return (parent == "vertex" ? QStringLiteral("Vertex ") : QStringLiteral("Segment ")) +
               qs(name);
    }
    return name.empty() ? QStringLiteral("(unnamed)") : qs(name);
}

PropertyLevel outlineLevel(const SubjectPtr& subject, const std::string& path);

PropertyRow outlineRow(const SubjectPtr& subject, const katana::cad::PropertyOutlineEntry& entry,
                       std::string_view parent)
{
    PropertyRow row;
    row.key = qs(entry.name);
    row.label = entryLabel(parent, entry.name);
    if (entry.value) {
        const QString text = qs(katana::entity::toString(*entry.value));
        row.value = shown(text);
        row.tip =
            tipFor(qs(entry.path), text + " (" + qs(katana::entity::typeName(*entry.value)) + ")");
        // The per-vertex heights are shown on the vertices, where they
        // belong, rather than as a line of a thousand numbers.
        if (subject->verticesShown > 0 && !subject->metadata &&
            entry.path == katana::entity::kElevationsProperty) {
            row.value = QStringLiteral("see Vertices");
            row.muted = true;
        }
    } else if (entry.varies) {
        row.value = QStringLiteral("<varies>");
        row.muted = true;
        row.tip =
            tipFor(qs(entry.path), QString("held by %1 of the %2 selected, not all alike")
                                       .arg(grouped(entry.holders), grouped(subject->ids.size())));
    } else if (entry.children > 0) {
        row.value = countOf(entry.values, "value", "values");
        row.muted = true;
    }
    if (entry.children > 0) {
        row.children = [subject, path = entry.path] { return outlineLevel(subject, path); };
    }
    return row;
}

PropertyLevel outlineLevel(const SubjectPtr& subject, const std::string& path)
{
    const std::vector<const PropertyMap*> maps = subject->maps();
    auto entries = std::make_shared<std::vector<katana::cad::PropertyOutlineEntry>>(
        katana::cad::propertyOutline(maps, path));
    // The vertices' own attributes are under Vertices, beside their places -
    // unless some name no vertex the list has (a vertex since removed), when
    // the branch stays here too, so nothing held is out of sight.
    if (path.empty() && subject->verticesShown > 0 && !subject->metadata) {
        const auto listed = [&](const katana::cad::PropertyOutlineEntry& vertex) {
            const auto number = katana::core::parseInteger(vertex.name);
            return number && *number >= 1 &&
                   static_cast<std::uint64_t>(*number) <= subject->verticesShown;
        };
        const bool allListed =
            std::ranges::all_of(katana::cad::propertyOutline(maps, "vertex"), listed);
        if (allListed) {
            std::erase_if(*entries, [](const auto& entry) { return entry.path == "vertex"; });
        }
    }
    PropertyLevel level;
    level.count = entries->size();
    level.row = [subject, entries, path](std::size_t i) {
        return outlineRow(subject, (*entries)[i], path);
    };
    return level;
}

// The vertices of a geometry that is a list of them; empty for any other.
std::vector<Point2> verticesOf(const katana::entity::Geometry& geometry)
{
    if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&geometry)) {
        return polyline->vertices;
    }
    if (const auto* leader = std::get_if<katana::entity::LeaderGeometry>(&geometry)) {
        return leader->vertices;
    }
    return {};
}

PropertyLevel vertexLevel(const SubjectPtr& subject)
{
    const Entity* entity = subject->ids.empty()
                               ? nullptr
                               : subject->document->model().entities.find(subject->ids.front());
    if (entity == nullptr) {
        return {};
    }
    auto points = std::make_shared<const std::vector<Point2>>(verticesOf(entity->geometry));
    auto heights = std::make_shared<const std::vector<std::optional<double>>>(
        katana::entity::heightsOf(entity->properties, points->size()));
    PropertyLevel level;
    level.count = points->size();
    level.row = [subject, points, heights](std::size_t i) {
        PropertyRow row;
        // Numbered from 1, as the archive numbers the attributes of each
        // (vertex/1/...) and as a person counts them.
        const std::string number = std::to_string(i + 1);
        row.key = qs(number);
        row.label = "Vertex " + row.key;
        row.value = planPoint((*points)[i]);
        // Absent is not zero: a vertex with no height shows none.
        if (const std::optional<double>& height = (*heights)[i]) {
            row.value += ", " + planNumber(*height);
        }
        const std::string path = "vertex/" + number;
        const Entity* now = subject->document->model().entities.find(subject->ids.front());
        if (now != nullptr && katana::cad::hasPropertiesUnder(now->properties, path)) {
            row.children = [subject, path] { return outlineLevel(subject, path); };
        }
        return row;
    };
    return level;
}

// What the geometry of one entity says, as rows.
std::vector<PropertyRow> geometryRows(const katana::entity::Model& model,
                                      const katana::entity::Geometry& geometry)
{
    using Rows = std::vector<PropertyRow>;
    struct Visitor {
        // A label's words are worked out from its target, which is in the
        // model, not in the label.
        const katana::entity::Model& model;

        Rows operator()(const katana::entity::PointGeometry& g) const
        {
            return {leaf("Position", planPoint(g.position))};
        }
        Rows operator()(const katana::geometry::Segment2& g) const
        {
            const double degrees =
                katana::math::normalizeAngle(g.delta().angle()) * katana::math::kRadToDeg;
            return {leaf("Start", planPoint(g.start)), leaf("End", planPoint(g.end)),
                    leaf("Length", planNumber(g.length())),
                    leaf("Angle", planNumber(degrees) + " deg")};
        }
        Rows operator()(const katana::geometry::Arc2& g) const
        {
            return {
                leaf("Centre", planPoint(g.center)), leaf("Radius", planNumber(g.radius)),
                leaf("Start angle", planNumber(g.startAngle * katana::math::kRadToDeg) + " deg"),
                leaf("Sweep", planNumber(g.sweep * katana::math::kRadToDeg) + " deg"),
                leaf("Length", planNumber(g.length()))};
        }
        Rows operator()(const katana::geometry::Polyline2& g) const
        {
            Rows rows = {leaf("Vertices", grouped(g.vertices.size())),
                         leaf("Closed", g.closed ? "yes" : "no"),
                         leaf("Length", planNumber(g.length()))};
            if (g.closed) {
                rows.push_back(leaf("Area", planNumber(g.area())));
            }
            return rows;
        }
        Rows operator()(const katana::geometry::Circle2& g) const
        {
            return {leaf("Centre", planPoint(g.center)), leaf("Radius", planNumber(g.radius)),
                    leaf("Circumference", planNumber(g.perimeter())),
                    leaf("Area", planNumber(g.area()))};
        }
        Rows operator()(const katana::entity::TextGeometry& g) const
        {
            Rows rows = {
                leaf("Position", planPoint(g.position)), leaf("Text", qs(g.text)),
                leaf("Height", planNumber(g.height)),
                leaf("Rotation", planNumber(g.rotation * katana::math::kRadToDeg) + " deg")};
            if (!g.style.empty()) {
                rows.push_back(leaf("Text style", qs(g.style)));
            }
            if (g.paperHeight > 0.0) {
                rows.push_back(leaf("Paper height", planNumber(g.paperHeight) + " mm"));
            }
            rows.push_back(leaf("Justify", qs(katana::entity::toString(g.justify))));
            return rows;
        }
        Rows operator()(const katana::entity::DimensionGeometry& g) const
        {
            Rows rows = {leaf("Kind", qs(katana::entity::toString(g.kind))),
                         leaf("Start", planPoint(g.start)), leaf("End", planPoint(g.end)),
                         leaf("Measurement", planNumber(g.measurement())),
                         leaf("Offset", planNumber(g.offset))};
            if (g.usesVertex()) {
                rows.push_back(leaf("Vertex", planPoint(g.vertex)));
            }
            if (g.startRef.associated() || g.endRef.associated() || g.vertexRef.associated()) {
                rows.push_back(leaf("Associative", "yes"));
            }
            return rows;
        }
        Rows operator()(const katana::entity::LabelGeometry& g) const
        {
            Rows rows = {leaf("Label style", qs(g.style))};
            if (g.target != 0) {
                rows.push_back(leaf("Labels", QString::number(g.target)));
            } else {
                rows.push_back(leaf("Alignment", qs(g.alignment)));
            }
            if (g.part >= 0) {
                rows.push_back(leaf("Part", QString::number(g.part)));
            }
            if (!g.rule.empty()) {
                rows.push_back(leaf("Rule", qs(g.rule)));
            }
            // What it says and where, as LABEL LIST replies them: the words
            // through the same resolver, so the two cannot disagree.
            rows.push_back(leaf("Text", qs(katana::cad::annotation::shownLabelText(model, g))));
            rows.push_back(leaf("Position", g.position ? "pinned at " + planPoint(*g.position)
                                                       : QString("automatic")));
            return rows;
        }
        Rows operator()(const katana::entity::LeaderGeometry& g) const
        {
            // The note as it is drawn: a smart leader's is read off the
            // entity its tip is on (docs/annotation.md, "Smart leaders").
            Rows rows = {leaf("Tip", planPoint(g.vertices.front())),
                         leaf("Vertices", grouped(g.vertices.size())),
                         leaf("Text", qs(katana::entity::leaderNote(
                                          model, g, katana::cad::codePropertyCandidates())))};
            if (g.fields) {
                rows.push_back(leaf("Template", qs(g.text)));
            }
            if (!g.labelStyle.empty()) {
                rows.push_back(leaf("Label style", qs(g.labelStyle)));
            }
            if (g.tipRef.associated()) {
                rows.push_back(leaf("On", QString::number(g.tipRef.entity) + " (" +
                                              qs(katana::entity::describe(g.tipRef)) + ")"));
            }
            rows.push_back(leaf("Callout", qs(katana::entity::toString(g.callout))));
            return rows;
        }
        // The drawing system's kinds; their vertices are edited in the
        // Vertices panel (qt/drawing/vertex_panel.hpp).
        Rows operator()(const katana::geometry::CurvePolyline2& g) const
        {
            Rows rows = {leaf("Vertices", grouped(g.vertices.size())),
                         leaf("Closed", g.closed ? "yes" : "no"),
                         leaf("Arcs", g.hasArcs() ? "yes" : "no"),
                         leaf("Heights", g.hasHeights() ? "yes" : "no"),
                         leaf("Length", planNumber(g.length()))};
            if (g.closed) {
                rows.push_back(leaf("Area", planNumber(g.area())));
            }
            return rows;
        }
        Rows operator()(const katana::geometry::Ellipse2& g) const
        {
            return {
                leaf("Centre", planPoint(g.center)),
                leaf("Major radius", planNumber(g.majorRadius())),
                leaf("Minor radius", planNumber(g.minorRadius())),
                leaf("Rotation", planNumber(g.majorAxis.angle() * katana::math::kRadToDeg) + " deg"),
                leaf("Sweep", planNumber(g.sweep * katana::math::kRadToDeg) + " deg"),
                leaf("Length", planNumber(g.length()))};
        }
        Rows operator()(const katana::geometry::Spline2& g) const
        {
            return {leaf("Degree", QString::number(g.degree)),
                    leaf("Control points", grouped(g.controlPoints.size())),
                    leaf("Fit points", grouped(g.fitPoints.size())),
                    leaf("Length", planNumber(g.length()))};
        }
    };
    return std::visit(Visitor{model}, geometry);
}

// The groups of one entity.
std::vector<PropertyRow> entityRows(const katana::cad::Document& document, const Entity& entity)
{
    std::vector<PropertyRow> rows;

    PropertyRow general = heading("General", "general");
    general.open = true;
    std::vector<PropertyRow> generalRows = {
        leaf("Id", QString::number(entity.id)),
        leaf("Type", qs(katana::entity::toString(entity.type()))), leaf("Layer", qs(entity.layer)),
        leaf("Colour", entity.color ? qs(entity.color->toHex()) : QStringLiteral("ByLayer"))};
    if (!entity.visible) {
        generalRows.push_back(leaf("Visible", "no"));
    }
    general.children = [generalRows] { return fixedLevel(generalRows); };
    rows.push_back(std::move(general));

    const std::size_t vertices = verticesOf(entity.geometry).size();
    PropertyRow geometry = heading("Geometry", "geometry");
    geometry.open = true;
    std::vector<PropertyRow> shapeRows = geometryRows(document.model(), entity.geometry);
    // Their count is the Vertices group's own, beside the list.
    if (vertices > 0) {
        std::erase_if(shapeRows, [](const PropertyRow& row) { return row.key == "Vertices"; });
    }
    geometry.children = [shapeRows] { return fixedLevel(shapeRows); };
    rows.push_back(std::move(geometry));

    auto subject = std::make_shared<Subject>();
    subject->document = &document;
    subject->ids = {entity.id};
    subject->verticesShown = vertices;
    if (vertices > 0) {
        PropertyRow list = heading("Vertices", "vertices", countOf(vertices, "vertex", "vertices"));
        list.tip =
            tipFor("Vertices", "Each vertex's position and height, with its own attributes beneath "
                               "it; read a page at a time when opened.");
        list.children = [subject] { return vertexLevel(subject); };
        rows.push_back(std::move(list));
    }

    if (!entity.properties.empty()) {
        PropertyRow attributes = heading("Attributes", "attributes",
                                         countOf(entity.properties.size(), "value", "values"));
        attributes.open = true;
        attributes.children = [subject] { return outlineLevel(subject, {}); };
        rows.push_back(std::move(attributes));
    }

    if (!entity.metadata.empty()) {
        auto source = std::make_shared<Subject>(*subject);
        source->metadata = true;
        PropertyRow record =
            heading("Source", "source", countOf(entity.metadata.size(), "value", "values"));
        record.tip = tipFor("Source", "Where the entity came from: what the file it was "
                                      "imported from said about it, kept to be written back.");
        record.children = [source] { return outlineLevel(source, {}); };
        rows.push_back(std::move(record));
    }
    return rows;
}

// Several entities: how many of each type, then what they hold, read as one.
std::vector<PropertyRow> selectionRows(const katana::cad::Document& document,
                                       const std::vector<EntityId>& ids)
{
    // Opened at once only up to here: past it the shared attributes are read
    // when a person asks, since reading them is a pass over every entity.
    constexpr std::size_t kOpenAttributesUpTo = 500;

    std::vector<PropertyRow> rows;
    PropertyRow general = heading("General", "general");
    general.open = true;
    std::vector<PropertyRow> generalRows = {
        leaf("Selection", countOf(ids.size(), "entity", "entities"))};
    std::map<QString, std::size_t> byType;
    bool anyProperties = false;
    for (const EntityId id : ids) {
        if (const Entity* entity = document.model().entities.find(id)) {
            ++byType[qs(katana::entity::toString(entity->type()))];
            anyProperties = anyProperties || !entity->properties.empty();
        }
    }
    for (const auto& [type, count] : byType) {
        generalRows.push_back(leaf(type, grouped(count)));
    }
    general.children = [generalRows] { return fixedLevel(generalRows); };
    rows.push_back(std::move(general));

    if (anyProperties) {
        auto subject = std::make_shared<Subject>();
        subject->document = &document;
        subject->ids = ids;
        PropertyRow attributes = heading("Attributes", "attributes", "shared, or <varies>");
        attributes.tip = tipFor("Attributes", "What the selected entities hold, read as one: a "
                                              "value where they all agree, else <varies>.");
        attributes.open = ids.size() <= kOpenAttributesUpTo;
        attributes.children = [subject] { return outlineLevel(subject, {}); };
        rows.push_back(std::move(attributes));
    }
    return rows;
}

// ASCII case folding, so a search of thirty thousand values makes no copy of
// each as a QString; a letter outside ASCII is matched as typed.
bool holds(std::string_view text, std::string_view folded)
{
    if (folded.empty()) {
        return true;
    }
    const auto at = std::ranges::search(
        text, folded, [](char a, char b) { return katana::core::asciiLower(a) == b; });
    return !at.empty();
}

} // namespace

PropertyLevel fixedLevel(std::vector<PropertyRow> rows)
{
    auto held = std::make_shared<const std::vector<PropertyRow>>(std::move(rows));
    PropertyLevel level;
    level.count = held->size();
    level.row = [held](std::size_t i) { return (*held)[i]; };
    return level;
}

PropertyLevel selectionLevel(const katana::cad::Document& document)
{
    const std::vector<EntityId> ids = document.selection().ids();
    if (ids.empty()) {
        return fixedLevel({leaf("Selection", "none"),
                           leaf("Entities", QString::number(document.model().entities.size()))});
    }
    if (ids.size() == 1) {
        if (const Entity* entity = document.model().entities.find(ids.front())) {
            return fixedLevel(entityRows(document, *entity));
        }
        return fixedLevel({leaf("Selection", "an entity that no longer exists")});
    }
    return fixedLevel(selectionRows(document, ids));
}

PropertyLevel matchingLevel(const katana::cad::Document& document, const QString& text)
{
    const std::vector<EntityId> ids = document.selection().ids();
    if (ids.empty()) {
        return fixedLevel({leaf("Filter", "select something to look through its properties")});
    }
    const std::string folded = katana::core::lowered(text.trimmed().toStdString());

    // Every matching name, with the selection's value for it: shared, or
    // varying when they differ or only some hold it.
    struct Found {
        std::size_t holders = 0;
        const katana::entity::PropertyValue* first = nullptr;
        bool differs = false;
    };
    std::map<std::pair<bool, std::string_view>, Found> found; // (source?, name)
    std::size_t searched = 0;
    for (const EntityId id : ids) {
        const Entity* entity = document.model().entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        for (const bool source : {false, true}) {
            for (const auto& [key, value] : source ? entity->metadata : entity->properties) {
                ++searched;
                if (!holds(key, folded) && !holds(katana::entity::toString(value), folded)) {
                    continue;
                }
                Found& entry = found[{source, key}];
                ++entry.holders;
                if (entry.first == nullptr) {
                    entry.first = &value;
                } else if (*entry.first != value) {
                    entry.differs = true;
                }
            }
        }
    }

    auto rows = std::make_shared<std::vector<PropertyRow>>();
    PropertyRow summary =
        heading("Matches", "matches",
                QString("%1 of %2 values hold \"%3\"")
                    .arg(grouped(found.size()), grouped(searched), text.trimmed()));
    rows->push_back(std::move(summary));
    std::vector<std::pair<std::pair<bool, std::string_view>, Found>> ordered(found.begin(),
                                                                             found.end());
    std::ranges::stable_sort(ordered, [](const auto& a, const auto& b) {
        if (a.first.first != b.first.first) {
            return !a.first.first; // properties before the source record
        }
        return katana::cad::naturalLess(a.first.second, b.first.second);
    });
    for (const auto& [name, entry] : ordered) {
        const QString label =
            (name.first ? QStringLiteral("Source: ") : QString()) + qs(name.second);
        const bool varies = entry.differs || entry.holders < ids.size();
        PropertyRow row = leaf(label, varies ? QStringLiteral("<varies>")
                                             : qs(katana::entity::toString(*entry.first)));
        row.muted = varies;
        row.tip = tipFor(label, varies ? QString("held by %1 of the %2 selected, not all alike")
                                             .arg(grouped(entry.holders), grouped(ids.size()))
                                       : qs(katana::entity::toString(*entry.first)));
        rows->push_back(std::move(row));
    }
    PropertyLevel level;
    level.count = rows->size();
    level.row = [rows](std::size_t i) { return (*rows)[i]; };
    return level;
}

// ---- the panel --------------------------------------------------------------------------

struct PropertyTreePanel::Impl {
    QLineEdit* filter = nullptr;
    QTreeView* tree = nullptr;
    PropertyTreeModel* model = nullptr;
    QTimer* filterDelay = nullptr;
    const katana::cad::Document* document = nullptr;

    // What was shown last, and which of its rows were open, by key path;
    // whether the rows are the tree rather than the filter's list, and where
    // the tree was scrolled to.
    std::vector<EntityId> subject;
    bool showingTree = false;
    int scroll = 0;
    QSet<QString> opened;
    // The groups that open by themselves which a person closed: kept closed
    // from one selection to the next, as a person expects of a panel.
    QSet<QString> closedGroups;

    [[nodiscard]] QString pathOf(const QModelIndex& index) const
    {
        QStringList keys;
        for (QModelIndex at = index; at.isValid(); at = at.parent()) {
            keys.prepend(at.data(kKeyRole).toString());
        }
        return keys.join(kPathSeparator);
    }

    void remember()
    {
        opened.clear();
        std::vector<QModelIndex> pending = {QModelIndex()};
        while (!pending.empty()) {
            const QModelIndex parent = pending.back();
            pending.pop_back();
            for (int row = 0; row < model->rowCount(parent); ++row) {
                const QModelIndex index = model->index(row, 0, parent);
                const bool open = tree->isExpanded(index);
                if (!parent.isValid()) {
                    const QString key = index.data(kKeyRole).toString();
                    const auto* node = model->nodeOf(index);
                    if (node->data.open && !open) {
                        closedGroups.insert(key);
                    } else if (open) {
                        closedGroups.remove(key);
                    }
                }
                if (open) {
                    opened.insert(pathOf(index));
                    pending.push_back(index);
                }
            }
        }
    }

    void open(const QModelIndex& index)
    {
        if (model->canFetchMore(index) && model->rowCount(index) == 0) {
            model->fetchMore(index);
        }
        tree->expand(index);
    }

    // The rows that were open, opened again where they still are; for a new
    // subject only its groups, as they were left.
    void restore(bool sameSubject)
    {
        for (int row = 0; row < model->rowCount({}); ++row) {
            const QModelIndex index = model->index(row, 0, {});
            const QString key = index.data(kKeyRole).toString();
            const bool byDefault = model->nodeOf(index)->data.open;
            if (byDefault ? !closedGroups.contains(key) : opened.contains(key)) {
                open(index);
            }
        }
        if (!sameSubject) {
            return;
        }
        QStringList deeper;
        for (const QString& path : opened) {
            if (path.contains(kPathSeparator)) {
                deeper << path;
            }
        }
        // Parents before children.
        std::ranges::sort(deeper, [](const QString& a, const QString& b) {
            return a.count(kPathSeparator) < b.count(kPathSeparator);
        });
        for (const QString& path : deeper) {
            QModelIndex at;
            bool found = true;
            for (const QString& key : path.split(kPathSeparator)) {
                if (at.isValid() && model->canFetchMore(at) && model->rowCount(at) == 0) {
                    model->fetchMore(at);
                }
                QModelIndex next;
                for (int row = 0; row < model->rowCount(at); ++row) {
                    const QModelIndex child = model->index(row, 0, at);
                    if (child.data(kKeyRole).toString() == key) {
                        next = child;
                        break;
                    }
                }
                if (!next.isValid()) {
                    found = false;
                    break;
                }
                at = next;
            }
            if (found) {
                open(at);
            }
        }
    }

    // The selection shown again: the tree as it was left where the subject
    // is the one shown before, else its groups as they were left; or, with
    // words in the filter, what holds them.
    void show()
    {
        if (document == nullptr) {
            return;
        }
        const std::vector<EntityId> ids = document->selection().ids();
        const bool sameSubject = ids == subject;
        if (showingTree) {
            remember();
            if (sameSubject) {
                scroll = tree->verticalScrollBar()->value();
            }
        }
        const QString text = filter->text().trimmed();
        model->reset(text.isEmpty() ? selectionLevel(*document) : matchingLevel(*document, text));
        showingTree = text.isEmpty();
        if (showingTree) {
            restore(sameSubject);
            // Laid out now, so the scroll bar has the range to be put back in.
            tree->doItemsLayout();
            tree->verticalScrollBar()->setValue(sameSubject ? scroll : 0);
        }
        subject = ids;
    }

    void copySelected() const
    {
        QStringList lines;
        for (const QModelIndex& index : tree->selectionModel()->selectedRows(0)) {
            if (index.data(kMoreRole).toBool()) {
                continue;
            }
            const QString value = index.siblingAtColumn(1).data().toString();
            lines << (value.isEmpty() ? index.data().toString()
                                      : index.data().toString() + "\t" + value);
        }
        if (!lines.isEmpty()) {
            QApplication::clipboard()->setText(lines.join('\n'));
        }
    }
};

PropertyTreePanel::PropertyTreePanel(QWidget* parent)
    : QWidget(parent), impl_(std::make_unique<Impl>())
{
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(4);

    impl_->filter = new QLineEdit(this);
    impl_->filter->setObjectName("propertyFilter");
    impl_->filter->setPlaceholderText("Filter properties and values...");
    impl_->filter->setClearButtonEnabled(true);
    impl_->filter->setToolTip(
        "<b>Filter</b><br>List every property of the selection whose name or value holds these "
        "words, however deep in the tree - a vertex's attributes too.");
    impl_->filter->setAccessibleName("Filter properties");
    impl_->filter->addAction(katana::qt::icon(Icon::ZoomTo), QLineEdit::LeadingPosition);
    layout->addWidget(impl_->filter);

    impl_->tree = new QTreeView(this);
    // Named as the table it replaced was, so --report and DRIVE's '?' read it.
    impl_->tree->setObjectName("propertyTable");
    impl_->model = new PropertyTreeModel(impl_->tree);
    impl_->tree->setModel(impl_->model);
    impl_->tree->setUniformRowHeights(true); // the rows are one line: a page lays out at once
    impl_->tree->setSelectionMode(QAbstractItemView::ExtendedSelection);
    impl_->tree->setSelectionBehavior(QAbstractItemView::SelectRows);
    impl_->tree->setEditTriggers(QAbstractItemView::NoEditTriggers);
    impl_->tree->setTextElideMode(Qt::ElideMiddle);
    impl_->tree->setIndentation(14);
    impl_->tree->setAlternatingRowColors(false);
    impl_->tree->header()->setStretchLastSection(true);
    impl_->tree->header()->setSectionResizeMode(0, QHeaderView::Interactive);
    impl_->tree->header()->resizeSection(0, 130);
    impl_->tree->setContextMenuPolicy(Qt::ActionsContextMenu);
    auto* copy = new QAction("Copy", impl_->tree);
    copy->setObjectName("propertyCopy");
    copy->setShortcut(QKeySequence::Copy);
    copy->setShortcutContext(Qt::WidgetShortcut);
    copy->setStatusTip("Copy the selected rows, name and value separated by a tab");
    impl_->tree->addAction(copy);
    layout->addWidget(impl_->tree, 1);

    Impl* impl = impl_.get();
    connect(copy, &QAction::triggered, this, [impl] { impl->copySelected(); });
    // A Show more row makes the next page of its level where it is clicked -
    // once the click is over, since the page takes the clicked row away.
    const auto showMore = [impl, this](const QModelIndex& index) {
        if (!index.data(kMoreRole).toBool()) {
            return;
        }
        const bool top = !index.parent().isValid();
        QTimer::singleShot(0, this, [impl, top, level = QPersistentModelIndex(index.parent())] {
            // A level refreshed away meanwhile is left alone.
            if (top || level.isValid()) {
                impl->model->showMore(level);
            }
        });
    };
    connect(impl_->tree, &QTreeView::clicked, this, showMore);
    connect(impl_->tree, &QTreeView::activated, this, showMore);
    // Typing waits for a pause: every keystroke would otherwise search the
    // whole selection again.
    impl_->filterDelay = new QTimer(this);
    impl_->filterDelay->setSingleShot(true);
    impl_->filterDelay->setInterval(200);
    connect(impl_->filterDelay, &QTimer::timeout, this, [impl] { impl->show(); });
    connect(impl_->filter, &QLineEdit::textChanged, this, [impl](const QString& text) {
        // Cleared: the tree back at once, as it was left.
        if (text.trimmed().isEmpty()) {
            impl->filterDelay->stop();
            impl->show();
            return;
        }
        impl->filterDelay->start();
    });
}

PropertyTreePanel::~PropertyTreePanel() = default;

void PropertyTreePanel::showSelection(const katana::cad::Document& document)
{
    if (impl_->document != &document) {
        impl_->subject.clear();
    }
    impl_->document = &document;
    impl_->show();
}

QTreeView* PropertyTreePanel::tree() const
{
    return impl_->tree;
}

QLineEdit* PropertyTreePanel::filter() const
{
    return impl_->filter;
}

} // namespace katana::qt
