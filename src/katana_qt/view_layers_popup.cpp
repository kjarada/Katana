#include "view_layers_popup.hpp"

#include <algorithm>
#include <functional>
#include <map>
#include <string>
#include <utility>

#include <QCheckBox>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QScreen>
#include <QToolButton>
#include <QTreeWidget>
#include <QTreeWidgetItemIterator>
#include <QVBoxLayout>

#include "katana/entity/layer_path.hpp"
#include "katana/interop/reference_data.hpp"
#include "theme.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

using katana::cad::ViewId;
using katana::cad::ViewState;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// Whether the DOCUMENT lets `path` show: the layer's inherited visibility or,
// for a path segment that is not itself a layer, that of its nearest ancestor
// that is. LayerDatabase::resolve answers "not shown" for a name it does not
// hold, which for an intermediate node would grey out everything beneath it.
bool documentShows(const katana::entity::LayerDatabase& layers, std::string_view path)
{
    while (!path.empty()) {
        if (layers.contains(path)) {
            return layers.effectivelyVisible(path);
        }
        path = katana::entity::layerParent(path);
    }
    return true;
}

std::string pathOf(const QTreeWidgetItem* item)
{
    return item->data(0, ViewLayersPopup::kPathRole).toString().toStdString();
}

// The reference layer's own switch, in the Reference Data panel. A view can
// hide a reference the panel shows, never show one it hides - the same
// subtraction as the layers.
bool referenceShown(const katana::interop::ReferenceData* reference,
                    katana::interop::ReferenceId id)
{
    if (reference == nullptr) {
        return false;
    }
    for (const auto& raster : reference->rasters()) {
        if (raster.id == id) {
            return raster.visible;
        }
    }
    for (const auto& cloud : reference->pointClouds()) {
        if (cloud.id == id) {
            return cloud.visible;
        }
    }
    return false;
}

QToolButton* textButton(QWidget* parent, const QString& text, const QString& tip)
{
    auto* button = new QToolButton(parent);
    button->setText(text);
    button->setToolButtonStyle(Qt::ToolButtonTextOnly);
    button->setToolTip(QString("<b>%1</b><br>%2").arg(text, tip));
    button->setAccessibleName(text);
    button->setAccessibleDescription(tip);
    return button;
}

} // namespace

ViewLayersPopup::ViewLayersPopup(ViewWorkspace& workspace, ViewId view, QWidget* parent)
    : QFrame(parent, Qt::Popup), workspace_(workspace), view_(view)
{
    setObjectName("ViewLayersPopup");
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(10, 8, 10, 10);
    layout->setSpacing(6);

    heading_ = new QLabel(this);
    heading_->setObjectName("ViewLayersHeading");
    QFont bold = heading_->font();
    bold.setBold(true);
    heading_->setFont(bold);
    // Said once, where the choice is made: the difference between this and
    // the Layers panel is the thing most likely to be misunderstood.
    auto* note = new QLabel("Hidden here only. The Layers panel hides a layer in every view "
                            "and saves it with the drawing.",
                            this);
    note->setObjectName("ViewLayersNote");
    note->setWordWrap(true);

    filter_ = new QLineEdit(this);
    filter_->setObjectName("ViewLayersFilter");
    filter_->setPlaceholderText("Filter layers");
    filter_->setClearButtonEnabled(true);

    layers_ = new QTreeWidget(this);
    layers_->setObjectName("ViewLayersTree");
    layers_->setColumnCount(1);
    layers_->setHeaderHidden(true);
    layers_->setUniformRowHeights(true);
    layers_->setSelectionMode(QAbstractItemView::SingleSelection);

    auto* buttons = new QHBoxLayout();
    buttons->setSpacing(4);
    auto* showAllButton =
        textButton(this, "Show All",
                   "Show every layer and reference layer the document shows, in this view.");
    showAllButton->setObjectName("ViewLayersShowAll");
    isolate_ = textButton(this, "Isolate",
                          "Show only the selected layer and what lies beneath it, in this view. "
                          "Replaces whatever this view hid.");
    isolate_->setObjectName("ViewLayersIsolate");
    hideOthers_ = textButton(this, "Hide Others",
                             "Hide every layer but the selected one and what lies beneath it, "
                             "keeping what this view already hides beneath it.");
    hideOthers_->setObjectName("ViewLayersHideOthers");
    buttons->addWidget(showAllButton);
    buttons->addWidget(isolate_);
    buttons->addWidget(hideOthers_);
    buttons->addStretch();

    // The view's own switch for the selection's ghosts (docs/desktop.md, "The
    // selection in every view"): here because it is about what this view
    // hides.
    ghosts_ = new QCheckBox("Show the selection on hidden layers", this);
    ghosts_->setObjectName("ViewLayersShowSelection");
    ghosts_->setToolTip(
        "<b>Show the selection on hidden layers</b><br>A selected feature on a layer this "
        "view hides is drawn here faint and dotted, so a selection made in another view "
        "shows where it is. It is never picked, snapped to, given grips or plotted here. "
        "VIEWS SET &lt;id&gt; ghosts=on|off on the command line.");

    referenceHeading_ = new QLabel("Reference data", this);
    referenceHeading_->setFont(bold);
    references_ = new QTreeWidget(this);
    references_->setObjectName("ViewReferencesTree");
    references_->setColumnCount(2);
    references_->setHeaderHidden(true);
    references_->setRootIsDecorated(false);
    references_->setUniformRowHeights(true);
    references_->setSelectionMode(QAbstractItemView::NoSelection);
    references_->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    references_->header()->setSectionResizeMode(1, QHeaderView::ResizeToContents);
    references_->header()->setStretchLastSection(false);

    layout->addWidget(heading_);
    layout->addWidget(note);
    layout->addWidget(filter_);
    layout->addWidget(layers_, 1);
    layout->addLayout(buttons);
    layout->addWidget(ghosts_);
    layout->addWidget(referenceHeading_);
    layout->addWidget(references_);
    setMinimumWidth(300);

    connect(filter_, &QLineEdit::textChanged, this, [this](const QString& text) { setFilter(text); });
    connect(layers_, &QTreeWidget::itemChanged, this,
            [this](QTreeWidgetItem* item, int) { onLayerChanged(item); });
    connect(references_, &QTreeWidget::itemChanged, this,
            [this](QTreeWidgetItem* item, int) { onReferenceChanged(item); });
    connect(layers_, &QTreeWidget::currentItemChanged, this, [this] { refreshStates(); });
    connect(showAllButton, &QToolButton::clicked, this, [this] { showAll(); });
    // clicked, not toggled: only the user's click runs the line, never the
    // box being set from the view.
    connect(ghosts_, &QCheckBox::clicked, this, [this](bool on) {
        workspace_.setSelectionGhosts(view_, on);
        refreshStates();
    });
    // The buttons are enabled only when there is a selection, so a failure
    // here is a layer deleted under an open popup - nothing to report but
    // the rebuild that shows it has gone.
    connect(isolate_, &QToolButton::clicked, this, [this] {
        if (!isolateSelected()) {
            rebuild();
        }
    });
    connect(hideOthers_, &QToolButton::clicked, this, [this] {
        if (!hideOthers()) {
            rebuild();
        }
    });

    rebuild();
}

ViewState* ViewLayersPopup::state() const { return workspace_.viewSet().find(view_); }

QTreeWidgetItem* ViewLayersPopup::itemFor(const QString& path) const
{
    for (QTreeWidgetItemIterator it(layers_); *it != nullptr; ++it) {
        if ((*it)->data(0, kPathRole).toString() == path) {
            return *it;
        }
    }
    return nullptr;
}

void ViewLayersPopup::rebuild()
{
    const bool was = std::exchange(updating_, true);
    const QString current = layers_->currentItem() != nullptr
                                ? layers_->currentItem()->data(0, kPathRole).toString()
                                : QString();
    layers_->clear();
    references_->clear();

    const ViewState* view = state();
    heading_->setText(view != nullptr ? QString("Layers in %1")
                                            .arg(QString::fromStdString(
                                                katana::cad::ViewSet::title(*view)))
                                      : QString("Layers"));

    const auto& database = workspace_.document().model().layers;
    std::map<std::string, QTreeWidgetItem*, std::less<>> items;
    // Every node is made after its parent. The names in order are NOT a
    // strict pre-order walk (layer_path.hpp: "a 2" sorts between "a" and
    // "a/b"), and a segment need not be a layer at all, so the parent is
    // looked up - or made - rather than assumed to be the last item made.
    const std::function<QTreeWidgetItem*(std::string_view)> node =
        [&](std::string_view path) -> QTreeWidgetItem* {
        if (const auto found = items.find(path); found != items.end()) {
            return found->second;
        }
        const std::string_view parentPath = katana::entity::layerParent(path);
        QTreeWidgetItem* parent = parentPath.empty() ? nullptr : node(parentPath);
        auto* item = parent != nullptr ? new QTreeWidgetItem(parent) : new QTreeWidgetItem(layers_);
        item->setText(0, QString::fromStdString(std::string(katana::entity::layerLeaf(path))));
        item->setData(0, kPathRole, QString::fromStdString(std::string(path)));
        if (!database.contains(path)) {
            QFont italic = item->font(0);
            italic.setItalic(true);
            item->setFont(0, italic);
        }
        items.emplace(std::string(path), item);
        return item;
    };
    for (const std::string& path : database.names()) {
        node(path);
    }
    layers_->expandAll();
    if (QTreeWidgetItem* again = itemFor(current)) {
        layers_->setCurrentItem(again);
    }
    // Tall enough to show every layer up to eighteen rows, so a drawing of
    // ordinary size needs no scrolling in a popup that closes on the first
    // click outside it; past that it scrolls. Never under eight rows, so the
    // popup does not jump in size between a small drawing and a large one.
    {
        const int rows = std::clamp(static_cast<int>(items.size()), 8, 18);
        const int row = layers_->sizeHintForRow(0) > 0 ? layers_->sizeHintForRow(0) : 22;
        layers_->setMinimumHeight(rows * row + 2 * layers_->frameWidth() + 4);
    }

    int count = 0;
    const auto add = [&](katana::interop::ReferenceId id, const std::string& name,
                         const QString& kind) {
        auto* item = new QTreeWidgetItem(references_);
        item->setText(0, QString::fromStdString(name));
        item->setText(1, kind);
        item->setData(0, kReferenceRole, QVariant::fromValue<qulonglong>(id));
        ++count;
    };
    if (const katana::interop::ReferenceData* reference = workspace_.referenceData()) {
        for (const auto& raster : reference->rasters()) {
            add(raster.id, raster.name, "Raster");
        }
        for (const auto& cloud : reference->pointClouds()) {
            add(cloud.id, cloud.name, "Point cloud");
        }
    }
    referenceHeading_->setVisible(count > 0);
    references_->setVisible(count > 0);
    if (count > 0) {
        // Tall enough for the rows there are, up to five: most drawings have
        // an image or two, and the layers are what the popup is for.
        const int row = references_->sizeHintForRow(0) > 0 ? references_->sizeHintForRow(0) : 22;
        references_->setFixedHeight(row * std::min(count, 5) + 2 * references_->frameWidth() + 2);
    }

    refreshStates();
    setFilter(filter_->text());
    updating_ = was;
}

void ViewLayersPopup::refreshStates()
{
    const ViewState* view = state();
    if (view == nullptr) {
        return;
    }
    const bool was = std::exchange(updating_, true);
    const auto& database = workspace_.document().model().layers;
    for (QTreeWidgetItemIterator it(layers_); *it != nullptr; ++it) {
        QTreeWidgetItem* item = *it;
        const std::string path = pathOf(item);
        const bool documentHides = !documentShows(database, path);
        const std::string_view parent = katana::entity::layerParent(path);
        const bool parentHidden = !parent.empty() && view->layers.hides(parent);
        const bool greyed = documentHides || parentHidden;
        item->setCheckState(0, view->layers.hidesDirectly(path) ? Qt::Unchecked : Qt::Checked);
        item->setFlags(greyed ? Qt::ItemFlags{}
                              : Qt::ItemIsEnabled | Qt::ItemIsSelectable | Qt::ItemIsUserCheckable);
        // Greyed by hand as well: the stylesheet's colour for tree text holds
        // in every state, so a disabled item is otherwise drawn like the rest.
        item->setData(0, Qt::ForegroundRole,
                      greyed ? QVariant(QBrush(theme::textDisabled())) : QVariant());
        QString tip = QString::fromStdString(path);
        if (!database.contains(path)) {
            tip += "\nNot a layer itself: hiding it hides the layers beneath it.";
        }
        if (documentHides) {
            tip += "\nHidden in every view by the Layers panel.";
        } else if (parentHidden) {
            tip += "\nHidden in this view by a parent layer.";
        }
        item->setToolTip(0, tip);
    }
    const katana::interop::ReferenceData* reference = workspace_.referenceData();
    for (int row = 0; row < references_->topLevelItemCount(); ++row) {
        QTreeWidgetItem* item = references_->topLevelItem(row);
        const auto id = static_cast<katana::interop::ReferenceId>(
            item->data(0, kReferenceRole).toULongLong());
        const bool shownByPanel = referenceShown(reference, id);
        item->setCheckState(0, view->hiddenReferences.contains(id) ? Qt::Unchecked : Qt::Checked);
        item->setFlags(shownByPanel ? Qt::ItemIsEnabled | Qt::ItemIsUserCheckable
                                    : Qt::ItemFlags{});
        for (const int column : {0, 1}) {
            item->setData(column, Qt::ForegroundRole,
                          shownByPanel ? QVariant() : QVariant(QBrush(theme::textDisabled())));
        }
        item->setToolTip(0, shownByPanel ? item->text(0)
                                         : item->text(0) +
                                               "\nHidden in every view by the Reference Data "
                                               "panel.");
    }
    ghosts_->setChecked(view->selectionGhosts);
    const QTreeWidgetItem* current = layers_->currentItem();
    const bool selectable = current != nullptr && current->flags().testFlag(Qt::ItemIsEnabled);
    isolate_->setEnabled(selectable);
    hideOthers_->setEnabled(selectable);
    updating_ = was;
}

void ViewLayersPopup::changed() { workspace_.viewSettingsChanged(view_); }

void ViewLayersPopup::onLayerChanged(QTreeWidgetItem* item)
{
    if (updating_ || item == nullptr) {
        return;
    }
    ViewState* view = state();
    if (view == nullptr) {
        close();
        return;
    }
    const std::string path = pathOf(item);
    const bool shown = item->checkState(0) == Qt::Checked;
    if (shown ? view->layers.show(path) : view->layers.hide(path)) {
        changed();
    }
    refreshStates();
}

void ViewLayersPopup::onReferenceChanged(QTreeWidgetItem* item)
{
    if (updating_ || item == nullptr) {
        return;
    }
    ViewState* view = state();
    if (view == nullptr) {
        close();
        return;
    }
    const auto id =
        static_cast<katana::interop::ReferenceId>(item->data(0, kReferenceRole).toULongLong());
    const bool changedAny = item->checkState(0) == Qt::Checked
                                ? view->hiddenReferences.erase(id) > 0
                                : view->hiddenReferences.insert(id).second;
    if (changedAny) {
        changed();
    }
    refreshStates();
}

void ViewLayersPopup::showAll()
{
    ViewState* view = state();
    if (view == nullptr) {
        return;
    }
    if (!view->layers.empty() || !view->hiddenReferences.empty()) {
        view->layers.clear();
        view->hiddenReferences.clear();
        changed();
    }
    refreshStates();
}

Status ViewLayersPopup::isolateSelected()
{
    ViewState* view = state();
    const QTreeWidgetItem* item = layers_->currentItem();
    if (view == nullptr || item == nullptr) {
        return makeError(ErrorCode::NotFound, "no layer is selected");
    }
    if (auto status =
            view->layers.isolate(pathOf(item), workspace_.document().model().layers.names());
        !status) {
        return status;
    }
    changed();
    refreshStates();
    return {};
}

Status ViewLayersPopup::hideOthers()
{
    ViewState* view = state();
    const QTreeWidgetItem* item = layers_->currentItem();
    if (view == nullptr || item == nullptr) {
        return makeError(ErrorCode::NotFound, "no layer is selected");
    }
    const std::string path = pathOf(item);
    // Strictly beneath: the selected layer itself is the one being kept.
    std::vector<std::string> keep;
    for (const std::string& hidden : view->layers.hidden()) {
        if (hidden != path && katana::entity::isLayerUnder(hidden, path)) {
            keep.push_back(hidden);
        }
    }
    if (auto status = view->layers.isolate(path, workspace_.document().model().layers.names());
        !status) {
        return status;
    }
    for (const std::string& hidden : keep) {
        view->layers.hide(hidden);
    }
    changed();
    refreshStates();
    return {};
}

void ViewLayersPopup::setFilter(const QString& text)
{
    const QString needle = text.trimmed();
    const std::function<bool(QTreeWidgetItem*)> apply = [&](QTreeWidgetItem* item) {
        bool below = false;
        for (int i = 0; i < item->childCount(); ++i) {
            below = apply(item->child(i)) || below;
        }
        const bool match = needle.isEmpty() ||
                           item->data(0, kPathRole).toString().contains(needle, Qt::CaseInsensitive);
        item->setHidden(!match && !below);
        return match || below;
    };
    for (int i = 0; i < layers_->topLevelItemCount(); ++i) {
        apply(layers_->topLevelItem(i));
    }
    if (!needle.isEmpty()) {
        layers_->expandAll();
    }
    for (int row = 0; row < references_->topLevelItemCount(); ++row) {
        QTreeWidgetItem* item = references_->topLevelItem(row);
        item->setHidden(!needle.isEmpty() && !item->text(0).contains(needle, Qt::CaseInsensitive));
    }
}

void ViewLayersPopup::popup(const QWidget* anchor)
{
    // The size it will really have: adjustSize honours the minimum width,
    // which sizeHint() does not, and a popup placed by its hint hung off the
    // right of the screen by the difference.
    adjustSize();
    QSize size = this->size();
    QPoint at;
    const QScreen* screen = nullptr;
    if (anchor != nullptr) {
        // Right-aligned under the button: the button is at the right of the
        // view's title bar, and a popup hanging off to its right would cover
        // the next view rather than this one.
        const QPoint below = anchor->mapToGlobal(QPoint(anchor->width(), anchor->height()));
        at = QPoint(below.x() - size.width(), below.y());
        screen = anchor->screen();
    }
    if (screen == nullptr) {
        screen = QGuiApplication::primaryScreen();
    }
    if (screen != nullptr) {
        const QRect available = screen->availableGeometry();
        size = size.boundedTo(available.size());
        if (anchor != nullptr && at.y() + size.height() > available.bottom()) {
            at.setY(anchor->mapToGlobal(QPoint(0, 0)).y() - size.height());
        }
        // Not std::clamp: a popup exactly as wide as the screen makes the
        // upper bound one less than the lower, which clamp may not be given.
        at.setX(std::max(available.left(), std::min(at.x(), available.right() + 1 - size.width())));
        at.setY(
            std::max(available.top(), std::min(at.y(), available.bottom() + 1 - size.height())));
    }
    resize(size);
    move(at);
    show();
    filter_->setFocus();
}

} // namespace katana::qt
