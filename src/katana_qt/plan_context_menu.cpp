#include "plan_context_menu.hpp"

#include <algorithm>
#include <utility>
#include <vector>

#include <QAction>

#include "katana/entity/layer_path.hpp"

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

QString verbArgument(const QString& word)
{
    const bool plain = !word.isEmpty() && std::none_of(word.begin(), word.end(), [](QChar c) {
        return c.isSpace() || c == '"';
    });
    return plain ? word : '"' + word + '"';
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
        QAction* item = addLine(*styles, qs(name), QStringLiteral("planContextStyle.") + qs(name),
                                QStringLiteral("STYLE APPLY ") + verbArgument(qs(name)));
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

    // INFO names one entity; for several, List says what each is.
    if (selected.size() == 1) {
        addLine(*this, QStringLiteral("Entity &Information"), QStringLiteral("planContextInfo"),
                QStringLiteral("INFO ") + QString::number(selected.front()));
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
                                  const QString& line)
{
    auto* item = menu.addAction(text);
    item->setObjectName(objectName);
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
        const QString line = QStringLiteral("CHLAYER ") + verbArgument(qs(name));
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
