#include "layer_manager.hpp"

#include <vector>

#include <QCheckBox>
#include <QColorDialog>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QInputDialog>
#include <QPushButton>
#include <QTableWidget>
#include <QVBoxLayout>

#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/layer_path.hpp"

namespace cmd = katana::commands;

namespace katana::qt {

namespace {

using katana::entity::Layer;

constexpr const char* kDefaultDimensionStyle = "(default)";

QColor toQColor(const katana::entity::Color& color)
{
    return QColor(color.r, color.g, color.b, color.a);
}

} // namespace

struct LayerManagerDialog::Impl {
    katana::cad::Document& document;
    LayerManagerDialog::Log log;

    QTableWidget* table = nullptr;
    QComboBox* linetypeBox = nullptr;
    QDoubleSpinBox* weightBox = nullptr;
    QPushButton* colourButton = nullptr;
    QComboBox* hatchBox = nullptr;
    QComboBox* dimensionBox = nullptr;
    QCheckBox* visibleBox = nullptr;
    QCheckBox* lockedBox = nullptr;
    katana::entity::Color pickedColour{};

    Impl(katana::cad::Document& d, LayerManagerDialog::Log l) : document(d), log(std::move(l)) {}

    bool run(katana::commands::CommandPtr command, const QString& done)
    {
        const auto status = document.execute(std::move(command));
        if (!status) {
            log(QString::fromStdString(status.error().describe()), true);
            return false;
        }
        log(done, false);
        return true;
    }
};

LayerManagerDialog::LayerManagerDialog(katana::cad::Document& document, Log log, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>(document, std::move(log)))
{
    setWindowTitle("Layers");
    resize(980, 700);

    auto* layout = new QVBoxLayout(this);
    impl_->table = new QTableWidget(this);
    impl_->table->setColumnCount(8);
    impl_->table->setHorizontalHeaderLabels(
        {"Layer", "On", "Locked", "Colour", "Linetype", "Weight", "Hatch", "Entities"});
    // Read-only: every change goes through the form and a command, so
    // nothing here can run one from inside a view's own signal - the shape
    // of the layer-panel crash recorded in docs/cad.md.
    impl_->table->setEditTriggers(QAbstractItemView::NoEditTriggers);
    impl_->table->setSelectionBehavior(QAbstractItemView::SelectRows);
    impl_->table->setSelectionMode(QAbstractItemView::SingleSelection);
    impl_->table->horizontalHeader()->setStretchLastSection(true);
    layout->addWidget(impl_->table, 1);

    auto* editor = new QGroupBox("Definition", this);
    editor->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Maximum);
    auto* form = new QFormLayout(editor);
    impl_->linetypeBox = new QComboBox(editor);
    form->addRow("Linetype", impl_->linetypeBox);
    impl_->weightBox = new QDoubleSpinBox(editor);
    impl_->weightBox->setRange(0.0, 10.0);
    impl_->weightBox->setDecimals(2);
    impl_->weightBox->setSingleStep(0.05);
    impl_->weightBox->setSuffix(" mm on paper");
    form->addRow("Line weight", impl_->weightBox);
    impl_->colourButton = new QPushButton("Choose...", editor);
    form->addRow("Colour", impl_->colourButton);
    impl_->hatchBox = new QComboBox(editor);
    form->addRow("Hatch", impl_->hatchBox);
    impl_->dimensionBox = new QComboBox(editor);
    form->addRow("Dimension style", impl_->dimensionBox);
    auto* flags = new QWidget(editor);
    auto* flagLayout = new QHBoxLayout(flags);
    flagLayout->setContentsMargins(0, 0, 0, 0);
    impl_->visibleBox = new QCheckBox("Visible", flags);
    impl_->lockedBox = new QCheckBox("Locked", flags);
    flagLayout->addWidget(impl_->visibleBox);
    flagLayout->addWidget(impl_->lockedBox);
    flagLayout->addStretch(1);
    form->addRow("", flags);
    layout->addWidget(editor);

    auto* buttons = new QWidget(this);
    auto* buttonLayout = new QHBoxLayout(buttons);
    buttonLayout->setContentsMargins(0, 0, 0, 0);
    auto* add = new QPushButton("New...", buttons);
    auto* child = new QPushButton("New Child...", buttons);
    auto* save = new QPushButton("Save Changes", buttons);
    auto* move = new QPushButton("Rename or Move...", buttons);
    auto* remove = new QPushButton("Delete", buttons);
    auto* assign = new QPushButton("Put Selection Here", buttons);
    auto* current = new QPushButton("Make Current", buttons);
    move->setToolTip("A layer name is a path, so renaming it to another path moves it, "
                     "with everything nested under it and the entities on them");
    remove->setToolTip("Refused while entities are on it or layers are under it");
    for (QPushButton* button : {add, child, save, move, remove, assign, current}) {
        buttonLayout->addWidget(button);
    }
    buttonLayout->addStretch(1);
    layout->addWidget(buttons);

    auto* close = new QDialogButtonBox(QDialogButtonBox::Close, this);
    connect(close, &QDialogButtonBox::rejected, this, &QDialog::reject);
    connect(close, &QDialogButtonBox::accepted, this, &QDialog::accept);
    layout->addWidget(close);

    const auto reload = [this] {
        reloadTable();
        loadSelectedLayer();
    };

    connect(impl_->table, &QTableWidget::itemSelectionChanged, this,
            [this] { loadSelectedLayer(); });
    connect(impl_->colourButton, &QPushButton::clicked, this, [this] {
        const QColor picked =
            QColorDialog::getColor(toQColor(impl_->pickedColour), this, "Layer Colour");
        if (picked.isValid()) {
            impl_->pickedColour = katana::entity::Color{
                static_cast<std::uint8_t>(picked.red()), static_cast<std::uint8_t>(picked.green()),
                static_cast<std::uint8_t>(picked.blue()),
                static_cast<std::uint8_t>(picked.alpha())};
            impl_->colourButton->setText(QString::fromStdString(impl_->pickedColour.toHex()));
        }
    });

    connect(add, &QPushButton::clicked, this, [this, reload] {
        bool accepted = false;
        const QString name = QInputDialog::getText(
            this, "New Layer", "Layer name (use / to nest, e.g. design/surface/tin1):",
            QLineEdit::Normal, {}, &accepted);
        if (!accepted || name.trimmed().isEmpty()) {
            return;
        }
        Layer layer;
        layer.name = name.trimmed().toStdString();
        if (impl_->run(cmd::createLayer(std::move(layer)), "Layer " + name.trimmed() + " created.")) {
            reload();
        }
    });
    connect(child, &QPushButton::clicked, this, [this, reload] {
        const std::string under = selectedLayer();
        if (under.empty()) {
            return;
        }
        bool accepted = false;
        const QString leaf = QInputDialog::getText(
            this, "New Nested Layer",
            QString("New layer under '%1':").arg(QString::fromStdString(under)), QLineEdit::Normal,
            {}, &accepted);
        if (!accepted || leaf.trimmed().isEmpty()) {
            return;
        }
        Layer layer;
        layer.name = katana::entity::joinLayerPath(under, leaf.trimmed().toStdString());
        const QString full = QString::fromStdString(layer.name);
        if (impl_->run(cmd::createLayer(std::move(layer)), "Layer " + full + " created.")) {
            reload();
        }
    });
    connect(save, &QPushButton::clicked, this, [this, reload] {
        const std::string name = selectedLayer();
        const Layer* stored = impl_->document.model().layers.find(name);
        if (stored == nullptr) {
            return;
        }
        Layer changed = *stored;
        changed.linetype = impl_->linetypeBox->currentText().toStdString();
        changed.lineWeight = impl_->weightBox->value();
        changed.color = impl_->pickedColour;
        changed.hatchPattern = impl_->hatchBox->currentText().toStdString();
        const QString dimension = impl_->dimensionBox->currentText();
        changed.dimensionStyle =
            dimension == kDefaultDimensionStyle ? std::string() : dimension.toStdString();
        changed.visible = impl_->visibleBox->isChecked();
        changed.locked = impl_->lockedBox->isChecked();
        if (impl_->run(cmd::updateLayer(std::move(changed)),
                       "Layer " + QString::fromStdString(name) + " updated.")) {
            reload();
        }
    });
    connect(move, &QPushButton::clicked, this, [this, reload] {
        const std::string from = selectedLayer();
        if (from.empty()) {
            return;
        }
        bool accepted = false;
        const QString to = QInputDialog::getText(
            this, "Rename or Move Layer", "New full path (use / to nest):", QLineEdit::Normal,
            QString::fromStdString(from), &accepted);
        if (!accepted || to.trimmed().isEmpty()) {
            return;
        }
        if (impl_->run(cmd::renameLayer(from, to.trimmed().toStdString()),
                       QString("Layer %1 is now %2; everything nested under it and the entities "
                               "on them came too.")
                           .arg(QString::fromStdString(from), to.trimmed()))) {
            reload();
        }
    });
    connect(remove, &QPushButton::clicked, this, [this, reload] {
        const std::string name = selectedLayer();
        if (name.empty()) {
            return;
        }
        if (impl_->run(cmd::deleteLayer(name),
                       "Layer " + QString::fromStdString(name) + " deleted.")) {
            reload();
        }
    });
    connect(assign, &QPushButton::clicked, this, [this] {
        const std::string name = selectedLayer();
        if (name.empty()) {
            return;
        }
        const auto ids = impl_->document.selection().ids();
        if (ids.empty()) {
            impl_->log("Select the entities to move first.", true);
            return;
        }
        if (impl_->run(cmd::setEntityLayer(ids, name),
                       QString("%1 entities moved to %2.")
                           .arg(ids.size())
                           .arg(QString::fromStdString(name)))) {
            reloadTable();
        }
    });
    connect(current, &QPushButton::clicked, this, [this] {
        const std::string name = selectedLayer();
        if (name.empty()) {
            return;
        }
        // Not a command: the current layer is session state, like the
        // selection, and is not on the undo stack.
        if (const auto status = impl_->document.setCurrentLayer(name); !status) {
            impl_->log(QString::fromStdString(status.error().describe()), true);
            return;
        }
        impl_->log("Current layer is now " + QString::fromStdString(name) + ".", false);
    });

    reload();
}

LayerManagerDialog::~LayerManagerDialog() = default;

void LayerManagerDialog::reloadTable()
{
    const katana::entity::Model& model = impl_->document.model();
    impl_->table->setRowCount(0);
    // names() is ascending by full path, which IS a pre-order walk of the
    // tree (layer_path.hpp), so the table reads as the tree does.
    for (const std::string& name : model.layers.names()) {
        const Layer* layer = model.layers.find(name);
        if (layer == nullptr) {
            continue;
        }
        const int row = impl_->table->rowCount();
        impl_->table->insertRow(row);
        const QStringList cells{
            QString::fromStdString(name),
            layer->visible ? "yes" : "no",
            layer->locked ? "yes" : "no",
            QString::fromStdString(layer->color.toHex()),
            QString::fromStdString(layer->linetype),
            QString::number(layer->lineWeight, 'f', 2),
            QString::fromStdString(layer->hatchPattern),
            QString::number(model.entities.countOnLayer(name))};
        for (int column = 0; column < cells.size(); ++column) {
            auto* item = new QTableWidgetItem(cells.at(column));
            if (column == 3) {
                item->setForeground(toQColor(layer->color));
            }
            impl_->table->setItem(row, column, item);
        }
    }

    const auto fill = [](QComboBox* box, const std::vector<std::string>& names,
                         const char* extra) {
        const QString was = box->currentText();
        box->clear();
        if (extra != nullptr) {
            box->addItem(extra);
        }
        for (const std::string& name : names) {
            box->addItem(QString::fromStdString(name));
        }
        box->setCurrentText(was);
    };
    fill(impl_->linetypeBox, model.linetypes.names(), nullptr);
    fill(impl_->hatchBox, model.hatchPatterns.names(), nullptr);
    fill(impl_->dimensionBox, model.dimensionStyles.names(), kDefaultDimensionStyle);
}

void LayerManagerDialog::loadSelectedLayer()
{
    const Layer* layer = impl_->document.model().layers.find(selectedLayer());
    if (layer == nullptr) {
        return;
    }
    impl_->linetypeBox->setCurrentText(QString::fromStdString(layer->linetype));
    impl_->weightBox->setValue(layer->lineWeight);
    impl_->pickedColour = layer->color;
    impl_->colourButton->setText(QString::fromStdString(layer->color.toHex()));
    impl_->hatchBox->setCurrentText(QString::fromStdString(layer->hatchPattern));
    impl_->dimensionBox->setCurrentText(layer->dimensionStyle.empty()
                                            ? QString(kDefaultDimensionStyle)
                                            : QString::fromStdString(layer->dimensionStyle));
    impl_->visibleBox->setChecked(layer->visible);
    impl_->lockedBox->setChecked(layer->locked);
}

std::string LayerManagerDialog::selectedLayer() const
{
    const int row = impl_->table->currentRow();
    if (row < 0 || impl_->table->item(row, 0) == nullptr) {
        return {};
    }
    return impl_->table->item(row, 0)->text().toStdString();
}

void LayerManagerDialog::showFirstRow()
{
    if (impl_->table->rowCount() > 0) {
        impl_->table->selectRow(0);
    }
}

} // namespace katana::qt
