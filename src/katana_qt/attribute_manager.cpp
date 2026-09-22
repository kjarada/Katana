#include "attribute_manager.hpp"

#include <map>
#include <set>
#include <string>
#include <vector>

#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QInputDialog>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"

namespace cmd = katana::commands;

namespace katana::qt {

namespace {

using katana::entity::EntityId;
using katana::entity::PropertyValue;

// The whole key lives on the item, so a leaf deep in the tree still knows
// the flat name the model stores it under.
constexpr int kKeyRole = Qt::UserRole + 1;

// Values that differ across the selection are shown as this rather than as
// one of them: a manager that showed the first entity's value and wrote it
// to all of them would quietly overwrite the others.
constexpr const char* kVaries = "<varies>";

std::vector<std::string> splitKey(const std::string& key)
{
    std::vector<std::string> parts;
    std::size_t start = 0;
    while (start <= key.size()) {
        const std::size_t slash = key.find('/', start);
        if (slash == std::string::npos) {
            parts.push_back(key.substr(start));
            break;
        }
        parts.push_back(key.substr(start, slash - start));
        start = slash + 1;
    }
    return parts;
}

} // namespace

struct AttributeManagerDialog::Impl {
    katana::cad::Document& document;
    AttributeManagerDialog::Log log;

    QTreeWidget* tree = nullptr;
    QLabel* subject = nullptr;
    QLineEdit* valueBox = nullptr;
    QComboBox* typeBox = nullptr;

    Impl(katana::cad::Document& d, AttributeManagerDialog::Log l)
        : document(d), log(std::move(l))
    {
    }

    [[nodiscard]] std::vector<EntityId> ids() const { return document.selection().ids(); }

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

    // Every property of the selection, with its value where they agree and
    // nullopt where they do not.
    [[nodiscard]] std::map<std::string, std::optional<PropertyValue>> merged() const
    {
        std::map<std::string, std::optional<PropertyValue>> all;
        std::map<std::string, std::size_t> counts;
        const auto selected = ids();
        for (const EntityId id : selected) {
            const katana::entity::Entity* entity = document.model().entities.find(id);
            if (entity == nullptr) {
                continue;
            }
            for (const auto& [key, value] : entity->properties) {
                ++counts[key];
                const auto found = all.find(key);
                if (found == all.end()) {
                    all.emplace(key, value);
                } else if (found->second && *found->second != value) {
                    found->second.reset();
                }
            }
        }
        // A property only some of them carry is as much a disagreement as
        // one they value differently.
        for (auto& [key, value] : all) {
            if (counts[key] != selected.size()) {
                value.reset();
            }
        }
        return all;
    }
};

AttributeManagerDialog::AttributeManagerDialog(katana::cad::Document& document, Log log,
                                               QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>(document, std::move(log)))
{
    setWindowTitle("Attributes");
    resize(720, 620);

    auto* layout = new QVBoxLayout(this);
    impl_->subject = new QLabel(this);
    layout->addWidget(impl_->subject);

    impl_->tree = new QTreeWidget(this);
    impl_->tree->setColumnCount(3);
    impl_->tree->setHeaderLabels({"Attribute", "Value", "Type"});
    // Read-only, edited through the form below: see style_manager.cpp for
    // why nothing here runs a command from inside a view's own signal.
    impl_->tree->setEditTriggers(QAbstractItemView::NoEditTriggers);
    impl_->tree->header()->setSectionResizeMode(0, QHeaderView::ResizeToContents);
    layout->addWidget(impl_->tree, 1);

    auto* form = new QFormLayout;
    impl_->valueBox = new QLineEdit(this);
    form->addRow("Value", impl_->valueBox);
    impl_->typeBox = new QComboBox(this);
    impl_->typeBox->addItems({"text", "integer", "real", "boolean"});
    form->addRow("Type", impl_->typeBox);
    layout->addLayout(form);

    auto* buttons = new QWidget(this);
    auto* buttonLayout = new QHBoxLayout(buttons);
    buttonLayout->setContentsMargins(0, 0, 0, 0);
    auto* add = new QPushButton("Add...", buttons);
    auto* save = new QPushButton("Save Value", buttons);
    auto* rename = new QPushButton("Rename...", buttons);
    auto* remove = new QPushButton("Remove", buttons);
    add->setToolTip("Add an attribute to every selected entity; use / to nest it in a group");
    rename->setToolTip("Rename it on every selected entity, keeping its value and type");
    for (QPushButton* button : {add, save, rename, remove}) {
        buttonLayout->addWidget(button);
    }
    buttonLayout->addStretch(1);
    layout->addWidget(buttons);

    auto* close = new QDialogButtonBox(QDialogButtonBox::Close, this);
    connect(close, &QDialogButtonBox::rejected, this, &QDialog::reject);
    connect(close, &QDialogButtonBox::accepted, this, &QDialog::accept);
    layout->addWidget(close);

    connect(impl_->tree, &QTreeWidget::itemSelectionChanged, this, [this] {
        const std::string key = selectedKey();
        if (key.empty()) {
            return;
        }
        const auto all = impl_->merged();
        const auto found = all.find(key);
        if (found == all.end() || !found->second) {
            impl_->valueBox->setText(kVaries);
            return;
        }
        impl_->valueBox->setText(QString::fromStdString(katana::entity::toString(*found->second)));
        impl_->typeBox->setCurrentText(
            QString::fromUtf8(katana::entity::typeName(*found->second).data(),
                              static_cast<int>(katana::entity::typeName(*found->second).size())));
    });

    const auto valueFromForm = [this]() -> katana::core::Result<PropertyValue> {
        const std::string text = impl_->valueBox->text().toStdString();
        const QString type = impl_->typeBox->currentText();
        if (type == "text") {
            return PropertyValue(text);
        }
        if (type == "integer") {
            bool ok = false;
            const qlonglong value = impl_->valueBox->text().toLongLong(&ok);
            if (!ok) {
                return katana::core::makeError(katana::core::ErrorCode::ParseFailure,
                                               "not an integer", text);
            }
            return PropertyValue(static_cast<std::int64_t>(value));
        }
        if (type == "real") {
            bool ok = false;
            const double value = impl_->valueBox->text().toDouble(&ok);
            if (!ok) {
                return katana::core::makeError(katana::core::ErrorCode::ParseFailure, "not a number",
                                               text);
            }
            return PropertyValue(value);
        }
        const QString folded = impl_->valueBox->text().trimmed().toLower();
        if (folded == "true" || folded == "1") {
            return PropertyValue(true);
        }
        if (folded == "false" || folded == "0") {
            return PropertyValue(false);
        }
        return katana::core::makeError(katana::core::ErrorCode::ParseFailure, "not a boolean", text);
    };

    connect(add, &QPushButton::clicked, this, [this, valueFromForm] {
        if (impl_->ids().empty()) {
            impl_->log("Select the entities to add an attribute to first.", true);
            return;
        }
        bool accepted = false;
        const QString key = QInputDialog::getText(
            this, "Add Attribute", "Name (use / to nest, e.g. Asset/Dimensions/Size):",
            QLineEdit::Normal, {}, &accepted);
        if (!accepted || key.trimmed().isEmpty()) {
            return;
        }
        const auto value = valueFromForm();
        if (!value) {
            impl_->log(QString::fromStdString(value.error().describe()), true);
            return;
        }
        if (impl_->run(cmd::setEntityProperty(impl_->ids(), key.trimmed().toStdString(), *value),
                       QString("Attribute %1 set on %2 entities.")
                           .arg(key.trimmed())
                           .arg(impl_->ids().size()))) {
            reloadTree();
        }
    });

    connect(save, &QPushButton::clicked, this, [this, valueFromForm] {
        const std::string key = selectedKey();
        if (key.empty()) {
            return;
        }
        const auto value = valueFromForm();
        if (!value) {
            impl_->log(QString::fromStdString(value.error().describe()), true);
            return;
        }
        if (impl_->run(cmd::setEntityProperty(impl_->ids(), key, *value),
                       QString("Attribute %1 set on %2 entities.")
                           .arg(QString::fromStdString(key))
                           .arg(impl_->ids().size()))) {
            reloadTree();
        }
    });

    connect(rename, &QPushButton::clicked, this, [this] {
        const std::string key = selectedKey();
        if (key.empty()) {
            return;
        }
        bool accepted = false;
        const QString to = QInputDialog::getText(this, "Rename Attribute", "New name:",
                                                 QLineEdit::Normal,
                                                 QString::fromStdString(key), &accepted);
        if (!accepted || to.trimmed().isEmpty()) {
            return;
        }
        if (impl_->run(cmd::renameEntityProperty(impl_->ids(), key, to.trimmed().toStdString()),
                       QString("Attribute %1 renamed to %2.")
                           .arg(QString::fromStdString(key), to.trimmed()))) {
            reloadTree();
        }
    });

    connect(remove, &QPushButton::clicked, this, [this] {
        const std::string key = selectedKey();
        if (key.empty()) {
            return;
        }
        if (impl_->run(cmd::removeEntityProperty(impl_->ids(), key),
                       QString("Attribute %1 removed from %2 entities.")
                           .arg(QString::fromStdString(key))
                           .arg(impl_->ids().size()))) {
            reloadTree();
        }
    });

    reloadTree();
}

AttributeManagerDialog::~AttributeManagerDialog() = default;

void AttributeManagerDialog::reloadTree()
{
    impl_->tree->clear();
    const auto selected = impl_->ids();
    impl_->subject->setText(
        selected.empty()
            ? QString("Nothing is selected: select entities in the drawing to see their attributes.")
            : QString("%1 entities selected.").arg(selected.size()));

    // Branches are made as they are first named. The keys are sorted (a
    // PropertyMap is ordered), so a parent is always made before its
    // children and one pass is enough - the same reason the layer tree
    // builds in one pass.
    std::map<std::string, QTreeWidgetItem*> branches;
    for (const auto& [key, value] : impl_->merged()) {
        const std::vector<std::string> parts = splitKey(key);
        QTreeWidgetItem* parent = nullptr;
        std::string path;
        for (std::size_t i = 0; i + 1 < parts.size(); ++i) {
            path += (path.empty() ? "" : "/") + parts[i];
            auto found = branches.find(path);
            if (found == branches.end()) {
                auto* branch = parent == nullptr ? new QTreeWidgetItem(impl_->tree)
                                                 : new QTreeWidgetItem(parent);
                branch->setText(0, QString::fromStdString(parts[i]));
                found = branches.emplace(path, branch).first;
            }
            parent = found->second;
        }
        auto* leaf = parent == nullptr ? new QTreeWidgetItem(impl_->tree)
                                       : new QTreeWidgetItem(parent);
        leaf->setText(0, QString::fromStdString(parts.back()));
        leaf->setText(1, value ? QString::fromStdString(katana::entity::toString(*value))
                               : QString(kVaries));
        if (value) {
            const auto type = katana::entity::typeName(*value);
            leaf->setText(2, QString::fromUtf8(type.data(), static_cast<int>(type.size())));
        }
        leaf->setData(0, kKeyRole, QString::fromStdString(key));
    }
}

std::string AttributeManagerDialog::selectedKey() const
{
    const auto items = impl_->tree->selectedItems();
    if (items.isEmpty()) {
        return {};
    }
    return items.front()->data(0, kKeyRole).toString().toStdString();
}

void AttributeManagerDialog::expandAll()
{
    impl_->tree->expandAll();
    if (impl_->tree->topLevelItemCount() > 0) {
        impl_->tree->setCurrentItem(impl_->tree->topLevelItem(0));
    }
}

} // namespace katana::qt
