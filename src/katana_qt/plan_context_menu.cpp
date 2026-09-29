#include "plan_context_menu.hpp"

#include <algorithm>
#include <tuple>
#include <utility>
#include <vector>

#include <QAction>

#include "command_word.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/polyline_vertices.hpp"

namespace katana::qt {

namespace {

QString qs(const std::string& text) { return QString::fromStdString(text); }

// The one value every selected entity shares, or "" when they differ.
template <class Read>
std::string sharedBy(const katana::cad::Document& document, Read read)
{
    std::optional<std::string> shared;
    for (const katana::entity::EntityId id : document.selection().ids()) {
        const katana::entity::Entity* entity = document.model().entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        const std::string value = read(*entity);
        if (!shared) {
            shared = value;
        } else if (*shared != value) {
            return {};
        }
    }
    return shared.value_or(std::string());
}

bool selectionHas(const katana::cad::Document& document, katana::entity::EntityType type)
{
    for (const katana::entity::EntityId id : document.selection().ids()) {
        const katana::entity::Entity* entity = document.model().entities.find(id);
        if (entity != nullptr && entity->type() == type) {
            return true;
        }
    }
    return false;
}

} // namespace

katana::core::Result<QString> namedLine(const QString& verb, const QString& name,
                                        const QString& what)
{
    auto word = commandWord(name, what);
    if (!word) {
        return word.error();
    }
    return verb + ' ' + *word;
}

PlanContextMenu::PlanContextMenu(PlanContextMenuContext context, QWidget* parent)
    : QMenu(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("planContextMenu"));
    const katana::cad::Document& document = *context_.document;
    const std::vector<katana::entity::EntityId> selected = document.selection().ids();

    if (selected.empty()) {
        // Nothing to act on: the things that make a selection, and the tool
        // Enter would repeat, named - AutoCAD's "Repeat LINE".
        if (!context_.lastToolId.empty() && context_.actions != nullptr) {
            if (auto* tool = context_.actions->findChild<QAction*>(qs(context_.lastToolId))) {
                QString name = tool->text();
                name.remove('&');
                auto* repeat = addAction(QStringLiteral("Repeat ") + name);
                repeat->setObjectName(QStringLiteral("planContextRepeat"));
                connect(repeat, &QAction::triggered, tool, &QAction::trigger);
                addSeparator();
            }
        }
        addExisting(*this, "editSelectAll");
        addExisting(*this, "editSelectById");
        addExisting(*this, "select.quick");
        addSeparator();
        addExisting(*this, "viewZoomExtents");
        return;
    }

    addGripItems();
    addExisting(*this, "editErase");
    addSeparator();
    for (const char* tool :
         {"modify.move", "modify.copy", "modify.rotate", "modify.scale", "modify.mirror",
          "modify.offset"}) {
        addExisting(*this, tool);
    }
    addSeparator();

    // Put on Layer, in the layer tree's shape: a drawing brought in from a
    // survey has hundreds of layers, and a flat list of them would run off
    // the screen. The layer every selected entity is on is ticked.
    QMenu* layers = addMenu(QStringLiteral("Put on &Layer"));
    layers->setObjectName(QStringLiteral("planContextLayer"));
    addLayerItems(*layers, {},
                  sharedBy(document, [](const katana::entity::Entity& e) { return e.layer; }));

    QMenu* styles = addMenu(QStringLiteral("St&yle"));
    styles->setObjectName(QStringLiteral("planContextStyle"));
    const std::string style =
        sharedBy(document, [](const katana::entity::Entity& e) { return e.style; });
    // "-" is STYLE APPLY's ByLayer; an empty style is how an entity wears it.
    const bool allByLayer = std::ranges::all_of(selected, [&](katana::entity::EntityId id) {
        const katana::entity::Entity* entity = document.model().entities.find(id);
        return entity == nullptr || entity->style.empty();
    });
    QAction* byLayer = addLine(*styles, QStringLiteral("ByLayer"),
                               QStringLiteral("planContextStyle.ByLayer"),
                               QStringLiteral("STYLE APPLY -"));
    byLayer->setCheckable(true);
    byLayer->setChecked(allByLayer);
    const std::vector<std::string> styleNames = document.model().styles.names();
    if (!styleNames.empty()) {
        styles->addSeparator();
    }
    for (const std::string& name : styleNames) {
        QAction* item =
            addLine(*styles, qs(name), QStringLiteral("planContextStyle.") + qs(name),
                    namedLine(QStringLiteral("STYLE APPLY"), qs(name), QStringLiteral("the style")));
        item->setCheckable(true);
        item->setChecked(!style.empty() && name == style);
    }

    QMenu* colours = addMenu(QStringLiteral("&Colour"));
    colours->setObjectName(QStringLiteral("planContextColour"));
    addLine(*colours, QStringLiteral("ByLayer"), QStringLiteral("planContextColour.ByLayer"),
            QStringLiteral("COLOR BYLAYER"));
    auto* choose = colours->addAction(QStringLiteral("C&hoose..."));
    choose->setObjectName(QStringLiteral("planContextColour.Choose"));
    // The dialog starts from the colour the selection shares, when it does.
    const std::string hex = sharedBy(document, [](const katana::entity::Entity& e) {
        return e.color ? e.color->toHex() : std::string();
    });
    connect(choose, &QAction::triggered, this, [this, hex] {
        if (!context_.chooseColour) {
            return;
        }
        const QColor initial = hex.empty() ? QColor(Qt::white) : QColor(qs(hex));
        if (const std::optional<QColor> colour = context_.chooseColour(initial)) {
            (void)context_.run(QStringLiteral("COLOR ") +
                               colour->name(QColor::HexRgb).toUpper());
        }
    });
    addExisting(*this, "editAttributes");
    addSeparator();

    // INFO names one entity; for several, List says what each is. #id, which
    // is always the entity: INFO 12 is a file when one is called 12.
    if (selected.size() == 1) {
        addLine(*this, QStringLiteral("Entity &Information"), QStringLiteral("planContextInfo"),
                QString(QStringLiteral("INFO #") + QString::number(selected.front())));
    }
    addExisting(*this, "inquiry.list");
    addSeparator();
    addExisting(*this, "select.similar");
    addExisting(*this, "select.quick");
    addExisting(*this, "formatGlobalModify");
    // The editors of what the selection holds, when the window has them.
    if (selectionHas(document, katana::entity::EntityType::Text)) {
        addExisting(*this, "annotateEditText");
    }
    if (selectionHas(document, katana::entity::EntityType::Label)) {
        addExisting(*this, "annotateEditLabel");
    }
    addSeparator();
    addExisting(*this, "editDeselect");
}

void PlanContextMenu::addGripItems()
{
    // A polyline's vertex or segment middle, right-clicked: what vertex
    // editing offers for THAT vertex or segment, before the selection's
    // items. The edits are verb lines, the tools start with the grip as their
    // handle, so each is what a person would type or pick.
    if (!context_.grip) {
        return;
    }
    const katana::cad::Grip grip = *context_.grip;
    const katana::entity::Entity* entity = context_.document->model().entities.find(grip.entity);
    const auto polyline =
        entity != nullptr ? katana::cad::readPolyline(*entity) : std::nullopt;
    if (!polyline) {
        return;
    }
    const QString id = QString::number(grip.entity);
    const auto refuse = [](QAction* item, const std::string& why) {
        item->setEnabled(false);
        item->setStatusTip(QString::fromStdString(why));
    };
    if (grip.kind == katana::cad::GripKind::Vertex && grip.index < polyline->vertices.size()) {
        const std::size_t v = grip.index;
        addSection(QString("Vertex %1 of polyline %2").arg(v).arg(id));
        QAction* remove = addLine(*this, QStringLiteral("&Delete Vertex"),
                                  QStringLiteral("planContextVertex.Delete"),
                                  QString("VERTEX DELETE %1 %2").arg(id).arg(v));
        if (auto deleted = katana::geometry::deleteVertex(*polyline, v); !deleted) {
            refuse(remove, deleted.error().message);
        }
        QAction* after = addLine(*this, QStringLiteral("&Insert Vertex After"),
                                 QStringLiteral("planContextVertex.InsertAfter"),
                                 QString("VERTEX INSERT %1 #%1.s%2 after=%2").arg(id).arg(v));
        if (v >= polyline->segmentCount()) {
            refuse(after, "the last vertex of an open polyline has no segment after it");
        }
        if (polyline->closed) {
            QAction* start = addLine(*this, QStringLiteral("Make It the &Start"),
                                     QStringLiteral("planContextVertex.Start"),
                                     QString("STARTVERTEX %1 %2").arg(id).arg(v));
            if (v == 0) {
                refuse(start, "vertex 0 is already the start");
            }
        }
        addTool(QStringLiteral("&Move Vertex..."), QStringLiteral("planContextVertex.Move"),
                "draw.vertex.move");
        addTool(QStringLiteral("Set &Height..."), QStringLiteral("planContextVertex.Height"),
                "draw.vertex.height");
        const auto corner = katana::geometry::checkCorner(*polyline, v);
        for (const auto& [text, name, tool] :
             {std::tuple{QStringLiteral("&Fillet Corner..."), QStringLiteral("planContextVertex.Fillet"),
                         "draw.vertex.fillet"},
              std::tuple{QStringLiteral("C&hamfer Corner..."),
                         QStringLiteral("planContextVertex.Chamfer"), "draw.vertex.chamfer"}}) {
            if (QAction* item = addTool(text, name, tool); item != nullptr && !corner) {
                refuse(item, corner.error().message);
            }
        }
        addTool(QStringLiteral("S&traighten From Here..."),
                QStringLiteral("planContextVertex.Straighten"), "draw.vertex.straighten");
    } else if (grip.kind == katana::cad::GripKind::SegmentMid &&
               grip.index < polyline->segmentCount()) {
        const std::size_t s = grip.index;
        addSection(QString("Segment %1 of polyline %2").arg(s).arg(id));
        addLine(*this, QStringLiteral("Add Vertex at the &Middle"),
                QStringLiteral("planContextSegment.AddMiddle"),
                QString("VERTEX INSERT %1 #%1.s%2 after=%2").arg(id).arg(s));
        addTool(QStringLiteral("&Insert Vertex..."), QStringLiteral("planContextSegment.Insert"),
                "draw.vertex.insert");
        addTool(QStringLiteral("Make an &Arc..."), QStringLiteral("planContextSegment.Arc"),
                "draw.vertex.arc");
        if (polyline->isArc(s)) {
            addLine(*this, QStringLiteral("Make &Straight"), QStringLiteral("planContextSegment.Line"),
                    QString("VERTEX SET %1 %2 bulge=0").arg(id).arg(s));
        }
    } else {
        return;
    }
    addSeparator();
}

QAction* PlanContextMenu::addTool(const QString& text, const QString& objectName,
                                  const std::string& toolId)
{
    if (!context_.startToolOn || !context_.grip) {
        return nullptr;
    }
    auto* item = addAction(text);
    item->setObjectName(objectName);
    item->setStatusTip(QString("Starts the tool on this %1")
                           .arg(context_.grip->kind == katana::cad::GripKind::Vertex
                                    ? QStringLiteral("vertex")
                                    : QStringLiteral("segment")));
    const katana::cad::Grip grip = *context_.grip;
    connect(item, &QAction::triggered, this,
            [this, grip, toolId] { context_.startToolOn(toolId, grip); });
    return item;
}

void PlanContextMenu::addExisting(QMenu& menu, const char* objectName)
{
    if (context_.actions == nullptr) {
        return;
    }
    if (auto* action = context_.actions->findChild<QAction*>(QString::fromLatin1(objectName))) {
        menu.addAction(action);
    }
}

QAction* PlanContextMenu::addLine(QMenu& menu, const QString& text, const QString& objectName,
                                  const katana::core::Result<QString>& written)
{
    auto* item = menu.addAction(text);
    item->setObjectName(objectName);
    if (!written) {
        // Shown, since the name is really there, but with nothing to run: a
        // line cut short at the quote would act on another name.
        item->setEnabled(false);
        item->setStatusTip(QString::fromStdString(written.error().message));
        return item;
    }
    const QString line = *written;
    // The line itself in the status bar, so a person learns the verb.
    item->setStatusTip(line);
    connect(item, &QAction::triggered, this, [this, line] {
        if (context_.run) {
            (void)context_.run(line); // what it said, refusals too, is in the log
        }
    });
    return item;
}

void PlanContextMenu::addLayerItems(QMenu& menu, const std::string& parent,
                                    const std::string& shared)
{
    const katana::entity::LayerDatabase& table = context_.document->model().layers;
    const std::vector<std::string> names = parent.empty() ? table.roots() : table.children(parent);
    for (const std::string& name : names) {
        const QString leaf = qs(std::string(katana::entity::layerLeaf(name)));
        const QString objectName = QStringLiteral("planContextLayer.") + qs(name);
        const auto line = namedLine(QStringLiteral("CHLAYER"), qs(name), QStringLiteral("the layer"));
        QMenu* target = &menu;
        if (table.hasChildren(name)) {
            // A layer with layers under it is a submenu of them, headed by
            // the layer itself, which entities can be on too.
            target = menu.addMenu(leaf);
            target->setObjectName(QStringLiteral("planContextLayerTree.") + qs(name));
        }
        QAction* item = addLine(*target, leaf, objectName, line);
        item->setCheckable(true);
        item->setChecked(name == shared);
        if (target != &menu) {
            target->addSeparator();
            addLayerItems(*target, name, shared);
        }
    }
}

} // namespace katana::qt
