#include "layer_manager.hpp"

#include <optional>
#include <utility>
#include <vector>

#include <QCheckBox>
#include <QColorDialog>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFont>
#include <QFormLayout>
#include <QFrame>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QIcon>
#include <QItemSelectionModel>
#include <QLabel>
#include <QLineEdit>
#include <QPalette>
#include <QPixmap>
#include <QPushButton>
#include <QSignalBlocker>
#include <QTableWidget>
#include <QVBoxLayout>

#include "customisation/customisation_context.hpp"
#include "customisation/document_watcher.hpp"
#include "customisation/name_picker.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "kept_name_combo.hpp"

namespace cmd = katana::commands;

namespace katana::qt {

namespace {

using katana::entity::Color;
using katana::entity::Layer;

constexpr const char* kDefaultDimensionStyle = "(default)";
const QColor kErrorColour{0xE0, 0x5A, 0x4F};

QColor toQColor(const Color& color) { return QColor(color.r, color.g, color.b, color.a); }

Color fromQColor(const QColor& colour)
{
    return Color{static_cast<std::uint8_t>(colour.red()), static_cast<std::uint8_t>(colour.green()),
                 static_cast<std::uint8_t>(colour.blue()),
                 static_cast<std::uint8_t>(colour.alpha())};
}

QIcon swatch(const Color& colour)
{
    QPixmap square(14, 14);
    square.fill(toQColor(colour));
    return QIcon(square);
}

std::string toName(const QString& text)
{
    return std::string(katana::core::trimmed(text.toStdString()));
}

// A button that Enter in a field of the dialog never presses: QDialog gives
// every QPushButton autoDefault, and Enter in the prompt's name would
// otherwise also press whichever button last had focus.
QPushButton* button(const QString& text, const char* objectName, const QString& tip,
                    QWidget* parent)
{
    auto* made = new QPushButton(text, parent);
    made->setObjectName(QString::fromLatin1(objectName));
    made->setAutoDefault(false);
    made->setDefault(false);
    if (!tip.isEmpty()) {
        made->setToolTip(tip);
    }
    return made;
}

} // namespace

struct LayerManagerDialog::Impl {
    LayerManagerDialog* dialog = nullptr;
    LayerManagerDialog::Log log;

    // The pickers read the Document through this; it has no thumbnails, so
    // the linetype list has no pictures here.
    CustomisationContext context{};
    QTableWidget* table = nullptr;
    // A NamePicker, not a plain combo: a layer may name a linestyle no
    // loaded library defines, and a combo that cannot show it wrote the
    // previous row's name back on Save (audit QT-02's twin).
    NamePicker* linetypeBox = nullptr;
    QDoubleSpinBox* weightBox = nullptr;
    QLineEdit* colourText = nullptr;
    QPushButton* colourButton = nullptr;
    QColorDialog* colourDialog = nullptr;
    QComboBox* hatchBox = nullptr;
    QComboBox* dimensionBox = nullptr;
    QCheckBox* visibleBox = nullptr;
    QCheckBox* lockedBox = nullptr;
    QFrame* prompt = nullptr;
    QLabel* promptLabel = nullptr;
    QLineEdit* promptName = nullptr;
    std::function<void(const std::string&)> promptAccept{};
    QLabel* statusLine = nullptr;

    Color pickedColour{};
    // What a person changed since the form was loaded: a spin box shows a
    // weight rounded to its decimals, and writing that back unasked would
    // change a layer nobody edited.
    bool weightEdited = false;
    bool colourEdited = false;
    bool loading = false;
    bool orphan = false;
    // The layer as the form was loaded from it. A reload leaves the form -
    // and its unsaved edits - alone while the stored layer still equals this.
    std::optional<Layer> loaded{};
    // What the next reload selects, after a command this dialog ran.
    std::optional<std::string> selectAfterReload{};

    // The receiver of every connection to a child widget: destroyed with the
    // Impl, so no widget signal can reach a destroyed Impl while the dialog's
    // children are deleted after it.
    std::unique_ptr<QObject> guard = std::make_unique<QObject>();
    // Last, so it goes first: no delivery reaches a half-destroyed Impl.
    std::unique_ptr<DocumentWatcher> watcher{};

    Impl(LayerManagerDialog* owner, katana::cad::Document& d, LayerManagerDialog::Log l)
        : dialog(owner), log(std::move(l))
    {
        context.document = &d;
        context.log = log;
    }

    template <typename Sender, typename Signal, typename Slot>
    void connectGuarded(Sender* sender, Signal signal, Slot slot)
    {
        QObject::connect(sender, signal, guard.get(), std::move(slot));
    }

    // ---- the Document, which may have gone ---------------------------------------

    [[nodiscard]] bool alive() const { return watcher == nullptr || watcher->documentAlive(); }

    // alive(), and when the Document has gone, the dialog shut down as well.
    // What a person can reach asks this before it reads the Document.
    [[nodiscard]] bool live()
    {
        if (alive()) {
            return true;
        }
        orphaned();
        return false;
    }

    // Once: every control but Close is disabled and the prompt and colour
    // dialog close, so nothing left on screen can reach the Document. Said on
    // the status line, not through log: that hook belongs to whoever owned
    // the Document, and may have gone with it.
    void orphaned()
    {
        if (orphan) {
            return;
        }
        orphan = true;
        prompt->hide();
        promptAccept = nullptr;
        if (colourDialog != nullptr) {
            colourDialog->hide();
        }
        table->setEnabled(false);
        for (QWidget* each : dialog->findChildren<QWidget*>()) {
            if (qobject_cast<QPushButton*>(each) != nullptr ||
                qobject_cast<QLineEdit*>(each) != nullptr ||
                qobject_cast<QComboBox*>(each) != nullptr ||
                qobject_cast<QAbstractSpinBox*>(each) != nullptr ||
                qobject_cast<QCheckBox*>(each) != nullptr) {
                each->setEnabled(each->objectName() == QStringLiteral("closeButton"));
            }
        }
        setStatus("The drawing this dialog edited has closed: nothing here acts on anything "
                  "any more.",
                  false);
    }

    [[nodiscard]] katana::cad::Document& document() const { return *context.document; }

    void setStatus(const QString& message, bool isError)
    {
        statusLine->setText(message);
        QPalette palette = statusLine->palette();
        palette.setColor(QPalette::WindowText,
                         isError ? kErrorColour : dialog->palette().color(QPalette::WindowText));
        statusLine->setPalette(palette);
    }

    void say(const QString& message, bool isError)
    {
        setStatus(message, isError);
        if (log && alive()) {
            log(message, isError);
        }
    }

    // Runs a command and says what it did, or why not. The reload that shows
    // its effect comes from the watcher, once, from the event loop.
    bool run(cmd::CommandPtr command, const QString& done)
    {
        if (!live()) {
            return false;
        }
        const auto status = document().execute(std::move(command));
        if (!status) {
            say(QString::fromStdString(status.error().describe()), true);
            return false;
        }
        say(done, false);
        return true;
    }

    // ---- the table and the form ------------------------------------------------------

    [[nodiscard]] std::string selectedLayer() const
    {
        const QModelIndexList rows = table->selectionModel()->selectedRows(0);
        if (rows.isEmpty()) {
            return {};
        }
        const QTableWidgetItem* item = table->item(rows.front().row(), 0);
        return item == nullptr ? std::string()
                               : item->data(Qt::UserRole).toByteArray().toStdString();
    }

    void selectRowOf(const std::string& name)
    {
        for (int row = 0; row < table->rowCount(); ++row) {
            const QTableWidgetItem* item = table->item(row, 0);
            if (item != nullptr && item->data(Qt::UserRole).toByteArray().toStdString() == name) {
                table->selectRow(row);
                table->scrollToItem(item);
                return;
            }
        }
        table->clearSelection();
    }

    // Rebuilds the table and the lists from the Document, keeping the
    // selected layer (or selecting the one a command just made), and the
    // form's edits unless its layer changed underneath them.
    void reload()
    {
        if (!alive()) {
            return;
        }
        const std::string select = selectAfterReload.value_or(selectedLayer());
        selectAfterReload.reset();
        const katana::entity::Model& model = document().model();
        {
            const QSignalBlocker quiet(table);
            table->setRowCount(0);
            // names() is ascending by full path, which IS a pre-order walk of
            // the tree (layer_path.hpp), so the table reads as the tree does.
            for (const std::string& name : model.layers.names()) {
                const Layer* layer = model.layers.find(name);
                if (layer == nullptr) {
                    continue;
                }
                const int row = table->rowCount();
                table->insertRow(row);
                const QStringList cells{QString::fromStdString(name),
                                        layer->visible ? "yes" : "no",
                                        layer->locked ? "yes" : "no",
                                        QString::fromStdString(layer->color.toHex()),
                                        QString::fromStdString(layer->linetype),
                                        QString::number(layer->lineWeight, 'f', 2),
                                        QString::fromStdString(layer->hatchPattern),
                                        QString::number(model.entities.countOnLayer(name))};
                for (int column = 0; column < cells.size(); ++column) {
                    auto* item = new QTableWidgetItem(cells.at(column));
                    if (column == 0) {
                        // The exact bytes, whatever the cell shows.
                        item->setData(Qt::UserRole, kept::bytes(name));
                        if (name == document().currentLayer()) {
                            QFont bold = item->font();
                            bold.setBold(true);
                            item->setFont(bold);
                            item->setToolTip("The current layer: new work is drawn on it");
                        }
                    }
                    if (column == 3) {
                        item->setIcon(swatch(layer->color));
                    }
                    table->setItem(row, column, item);
                }
            }
            selectRowOf(select);
        }

        // Each list is rebuilt from the model and then shows what it showed,
        // kept if the model no longer lists it.
        const auto fill = [](QComboBox* box, const std::vector<std::string>& names,
                             const char* extra) {
            const std::optional<std::string> was = kept::current(box);
            std::vector<kept::Choice> choices;
            if (extra != nullptr) {
                choices.push_back({QString::fromLatin1(extra), std::string()});
            }
            for (const std::string& name : names) {
                choices.push_back({QString::fromStdString(name), name});
            }
            kept::fill(box, choices);
            kept::show(box, was.value_or(std::string()));
        };
        linetypeBox->refresh();
        fill(hatchBox, model.hatchPatterns.names(), nullptr);
        fill(dimensionBox, model.dimensionStyles.names(), kDefaultDimensionStyle);

        const Layer* stored = model.layers.find(selectedLayer());
        if (stored == nullptr) {
            loaded.reset();
            return;
        }
        if (!loaded || *loaded != *stored) {
            loadForm(*stored);
        }
    }

    void loadForm(const Layer& layer)
    {
        loading = true;
        linetypeBox->setCurrentName(layer.linetype);
        weightBox->setValue(layer.lineWeight);
        showColour(layer.color);
        kept::show(hatchBox, layer.hatchPattern);
        kept::show(dimensionBox, layer.dimensionStyle);
        visibleBox->setChecked(layer.visible);
        lockedBox->setChecked(layer.locked);
        weightEdited = false;
        colourEdited = false;
        loaded = layer;
        loading = false;
    }

    void loadSelected()
    {
        if (!live()) {
            return;
        }
        const Layer* layer = document().model().layers.find(selectedLayer());
        if (layer == nullptr) {
            loaded.reset();
            return;
        }
        loadForm(*layer);
    }

    void showColour(const Color& colour)
    {
        pickedColour = colour;
        colourText->setText(QString::fromStdString(colour.toHex()));
        colourButton->setIcon(swatch(colour));
    }

    // A colour the person chose, by typing or in the colour dialog.
    void chooseColour(const Color& colour)
    {
        if (colour == pickedColour) {
            showColour(colour); // tidies the text: "#FF8000" for "#ff8000"
            return;
        }
        showColour(colour);
        colourEdited = true;
    }

    void save()
    {
        if (!live()) {
            return;
        }
        const std::string name = selectedLayer();
        const Layer* stored = document().model().layers.find(name);
        if (stored == nullptr) {
            say("Select a layer first.", true);
            return;
        }
        Layer changed = *stored;
        // Every name exactly as shown, which for a name no list holds is the
        // stored name itself, kept by its picker; numbers only when edited.
        changed.linetype = linetypeBox->currentName();
        if (weightEdited) {
            changed.lineWeight = weightBox->value();
        }
        if (colourEdited) {
            changed.color = pickedColour;
        }
        changed.hatchPattern = kept::current(hatchBox).value_or(stored->hatchPattern);
        changed.dimensionStyle = kept::current(dimensionBox).value_or(stored->dimensionStyle);
        changed.visible = visibleBox->isChecked();
        changed.locked = lockedBox->isChecked();
        if (changed == *stored) {
            // Not an edit, and so not an undo step (audit QT-01's shape).
            say("Nothing changed: layer " + QString::fromStdString(name) + " is as it was.",
                false);
            return;
        }
        if (run(cmd::updateLayer(std::move(changed)),
                "Layer " + QString::fromStdString(name) + " updated.")) {
            selectAfterReload = name;
        }
    }

    // ---- the prompt row ------------------------------------------------------------

    void askName(const QString& question, const QString& suggestion,
                 std::function<void(const std::string&)> accept)
    {
        if (!live()) {
            return;
        }
        promptLabel->setText(question);
        promptName->setText(suggestion);
        promptName->selectAll();
        promptAccept = std::move(accept);
        prompt->show();
        promptName->setFocus();
    }

    void acceptPrompt()
    {
        if (!live()) {
            return;
        }
        // Blanks either end of a typed path are never meant, and a layer
        // level may not start or end with one.
        const std::string answer = toName(promptName->text());
        if (answer.empty()) {
            say("Give a name first.", true);
            return;
        }
        prompt->hide();
        auto accept = std::move(promptAccept);
        promptAccept = nullptr;
        if (accept) {
            accept(answer);
        }
    }

    void build();
    void buildForm(QWidget* parent, QVBoxLayout* layout);
    void buildButtons(QWidget* parent, QVBoxLayout* layout);
    void buildPrompt(QWidget* parent, QVBoxLayout* layout);
};

void LayerManagerDialog::Impl::build()
{
    auto* layout = new QVBoxLayout(dialog);
    table = new QTableWidget(dialog);
    table->setObjectName(QStringLiteral("layerTable"));
    table->setColumnCount(8);
    table->setHorizontalHeaderLabels(
        {"Layer", "On", "Locked", "Colour", "Linetype", "Weight", "Hatch", "Entities"});
    // Read-only: every change goes through the form and a command, so
    // nothing here can run one from inside a view's own signal - the shape
    // of the layer-panel crash recorded in docs/cad.md.
    table->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table->setSelectionBehavior(QAbstractItemView::SelectRows);
    table->setSelectionMode(QAbstractItemView::SingleSelection);
    table->horizontalHeader()->setStretchLastSection(true);
    layout->addWidget(table, 1);
    connectGuarded(table, &QTableWidget::itemSelectionChanged, [this] { loadSelected(); });

    buildForm(dialog, layout);
    buildPrompt(dialog, layout);
    buildButtons(dialog, layout);

    statusLine = new QLabel(dialog);
    statusLine->setObjectName(QStringLiteral("layerStatus"));
    statusLine->setWordWrap(true);
    layout->addWidget(statusLine);

    auto* close = new QDialogButtonBox(dialog);
    QPushButton* closeButton = close->addButton(QDialogButtonBox::Close);
    closeButton->setObjectName(QStringLiteral("closeButton"));
    closeButton->setAutoDefault(false);
    connectGuarded(close, &QDialogButtonBox::rejected, [this] { dialog->reject(); });
    layout->addWidget(close);
}

void LayerManagerDialog::Impl::buildForm(QWidget* parent, QVBoxLayout* layout)
{
    auto* editor = new QGroupBox("Definition", parent);
    editor->setObjectName(QStringLiteral("layerForm"));
    editor->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Maximum);
    auto* form = new QFormLayout(editor);
    linetypeBox = new NamePicker(context, katana::cad::NameRole::Linetype, false, editor);
    linetypeBox->setObjectName(QStringLiteral("layerLinetype"));
    form->addRow("Linetype", linetypeBox);
    weightBox = new QDoubleSpinBox(editor);
    weightBox->setRange(0.0, 10.0);
    weightBox->setDecimals(2);
    weightBox->setSingleStep(0.05);
    weightBox->setSuffix(" mm on paper");
    weightBox->setObjectName(QStringLiteral("layerWeight"));
    form->addRow("Line weight", weightBox);

    auto* colourRow = new QWidget(editor);
    auto* colourLayout = new QHBoxLayout(colourRow);
    colourLayout->setContentsMargins(0, 0, 0, 0);
    colourText = new QLineEdit(colourRow);
    colourText->setObjectName(QStringLiteral("layerColourText"));
    colourText->setPlaceholderText("#RRGGBB");
    colourText->setToolTip("Type a colour as #RRGGBB (or #RRGGBBAA), or choose one");
    colourButton = button("Choose...", "layerColour", "Pick the colour in a colour dialog",
                          colourRow);
    colourLayout->addWidget(colourText, 1);
    colourLayout->addWidget(colourButton);
    form->addRow("Colour", colourRow);

    hatchBox = new QComboBox(editor);
    hatchBox->setObjectName(QStringLiteral("layerHatch"));
    form->addRow("Hatch", hatchBox);
    dimensionBox = new QComboBox(editor);
    dimensionBox->setObjectName(QStringLiteral("layerDimensionStyle"));
    form->addRow("Dimension style", dimensionBox);
    auto* flags = new QWidget(editor);
    auto* flagLayout = new QHBoxLayout(flags);
    flagLayout->setContentsMargins(0, 0, 0, 0);
    visibleBox = new QCheckBox("Visible", flags);
    visibleBox->setObjectName(QStringLiteral("layerVisible"));
    lockedBox = new QCheckBox("Locked", flags);
    lockedBox->setObjectName(QStringLiteral("layerLocked"));
    flagLayout->addWidget(visibleBox);
    flagLayout->addWidget(lockedBox);
    flagLayout->addStretch(1);
    form->addRow("", flags);
    layout->addWidget(editor);

    connectGuarded(weightBox, &QDoubleSpinBox::valueChanged, [this](double) {
        if (!loading) {
            weightEdited = true;
        }
    });
    // A typed colour is taken when the field is left or Enter is pressed;
    // one that is not a colour is refused and the field shows the last good
    // one again.
    connectGuarded(colourText, &QLineEdit::editingFinished, [this] {
        if (loading || !live()) {
            return;
        }
        const auto parsed = Color::fromHex(toName(colourText->text()));
        if (!parsed) {
            say("Not a colour: \"" + colourText->text() + "\". Type #RRGGBB, e.g. #FF8000.",
                true);
            showColour(pickedColour);
            return;
        }
        chooseColour(*parsed);
    });
    connectGuarded(colourButton, &QPushButton::clicked, [this] {
        if (!live()) {
            return;
        }
        // Window-modal but not blocking (open(), not exec()): the answer
        // arrives as a signal, and nothing waits on it.
        if (colourDialog == nullptr) {
            colourDialog = new QColorDialog(dialog);
            colourDialog->setObjectName(QStringLiteral("layerColourDialog"));
            colourDialog->setWindowTitle("Layer Colour");
            // Qt's own, so open() never waits on a platform dialog and a
            // test or a headless driver can find it by its objectName.
            colourDialog->setOption(QColorDialog::DontUseNativeDialog);
            connectGuarded(colourDialog, &QColorDialog::colorSelected,
                           [this](const QColor& picked) {
                               if (picked.isValid() && live()) {
                                   chooseColour(fromQColor(picked));
                               }
                           });
        }
        colourDialog->setCurrentColor(toQColor(pickedColour));
        colourDialog->open();
    });
}

void LayerManagerDialog::Impl::buildPrompt(QWidget* parent, QVBoxLayout* layout)
{
    prompt = new QFrame(parent);
    prompt->setObjectName(QStringLiteral("layerPrompt"));
    prompt->setFrameShape(QFrame::StyledPanel);
    auto* row = new QHBoxLayout(prompt);
    promptLabel = new QLabel(prompt);
    promptLabel->setObjectName(QStringLiteral("layerPromptLabel"));
    promptName = new QLineEdit(prompt);
    promptName->setObjectName(QStringLiteral("layerPromptName"));
    auto* ok = button("OK", "layerPromptOk", {}, prompt);
    auto* cancel = button("Cancel", "layerPromptCancel", {}, prompt);
    row->addWidget(promptLabel);
    row->addWidget(promptName, 1);
    row->addWidget(ok);
    row->addWidget(cancel);
    prompt->hide();
    layout->addWidget(prompt);
    connectGuarded(ok, &QPushButton::clicked, [this] { acceptPrompt(); });
    connectGuarded(promptName, &QLineEdit::returnPressed, [this] { acceptPrompt(); });
    connectGuarded(cancel, &QPushButton::clicked, [this] {
        prompt->hide();
        promptAccept = nullptr;
    });
}

void LayerManagerDialog::Impl::buildButtons(QWidget* parent, QVBoxLayout* layout)
{
    auto* buttons = new QWidget(parent);
    auto* row = new QHBoxLayout(buttons);
    row->setContentsMargins(0, 0, 0, 0);
    auto* add = button("New...", "layerNew", "A new layer; a name with / nests it", buttons);
    auto* child = button("New Child...", "layerNewChild", "A new layer under the selected one",
                         buttons);
    auto* saveButton = button("Save Changes", "layerSave", "Write the form to the layer", buttons);
    auto* revert = button("Revert", "layerRevert", "Put the form back as the layer is", buttons);
    auto* move = button("Rename or Move...", "layerMove",
                        "A layer name is a path, so renaming it to another path moves it, "
                        "with everything nested under it and the entities on them",
                        buttons);
    auto* remove = button("Delete", "layerDelete",
                          "Refused while entities are on it or layers are under it", buttons);
    auto* assign = button("Put Selection Here", "layerAssign",
                          "Move the entities selected in the drawing onto this layer", buttons);
    auto* current = button("Make Current", "layerCurrent", "Draw new work on this layer",
                           buttons);
    for (QPushButton* each : {add, child, saveButton, revert, move, remove, assign, current}) {
        row->addWidget(each);
    }
    row->addStretch(1);
    layout->addWidget(buttons);

    connectGuarded(add, &QPushButton::clicked, [this] {
        askName("New layer (use / to nest, e.g. design/surface/tin1):", {},
                [this](const std::string& name) {
                    Layer layer;
                    layer.name = name;
                    if (run(cmd::createLayer(std::move(layer)),
                            "Layer " + QString::fromStdString(name) + " created.")) {
                        selectAfterReload = name;
                    }
                });
    });
    connectGuarded(child, &QPushButton::clicked, [this] {
        if (!live()) {
            return;
        }
        const std::string under = selectedLayer();
        if (under.empty()) {
            say("Select the layer to nest the new one under first.", true);
            return;
        }
        askName(QString("New layer under %1:").arg(QString::fromStdString(under)), {},
                [this, under](const std::string& leaf) {
                    Layer layer;
                    layer.name = katana::entity::joinLayerPath(under, leaf);
                    const std::string full = layer.name;
                    if (run(cmd::createLayer(std::move(layer)),
                            "Layer " + QString::fromStdString(full) + " created.")) {
                        selectAfterReload = full;
                    }
                });
    });
    connectGuarded(saveButton, &QPushButton::clicked, [this] { save(); });
    connectGuarded(revert, &QPushButton::clicked, [this] { loadSelected(); });
    connectGuarded(move, &QPushButton::clicked, [this] {
        if (!live()) {
            return;
        }
        const std::string from = selectedLayer();
        if (from.empty()) {
            say("Select the layer to rename or move first.", true);
            return;
        }
        askName(QString("New full path for %1 (use / to nest):").arg(QString::fromStdString(from)),
                QString::fromStdString(from), [this, from](const std::string& to) {
                    if (to == from) {
                        say("Nothing changed: the path is the same.", false);
                        return;
                    }
                    if (run(cmd::renameLayer(from, to),
                            QString("Layer %1 is now %2; everything nested under it and the "
                                    "entities on them came too.")
                                .arg(QString::fromStdString(from), QString::fromStdString(to)))) {
                        selectAfterReload = to;
                    }
                });
    });
    connectGuarded(remove, &QPushButton::clicked, [this] {
        if (!live()) {
            return;
        }
        const std::string name = selectedLayer();
        if (name.empty()) {
            say("Select the layer to delete first.", true);
            return;
        }
        run(cmd::deleteLayer(name), "Layer " + QString::fromStdString(name) + " deleted.");
    });
    connectGuarded(assign, &QPushButton::clicked, [this] {
        if (!live()) {
            return;
        }
        const std::string name = selectedLayer();
        if (name.empty()) {
            say("Select the layer to move the entities onto first.", true);
            return;
        }
        // The drawing's selection as it is now, not as it was when the
        // dialog opened: a non-modal dialog sits beside the drawing.
        const auto ids = document().selection().ids();
        if (ids.empty()) {
            say("Select the entities to move first.", true);
            return;
        }
        if (run(cmd::setEntityLayer(ids, name), QString("%1 entities moved to %2.")
                                                    .arg(ids.size())
                                                    .arg(QString::fromStdString(name)))) {
            selectAfterReload = name;
        }
    });
    connectGuarded(current, &QPushButton::clicked, [this] {
        if (!live()) {
            return;
        }
        const std::string name = selectedLayer();
        if (name.empty()) {
            say("Select the layer to make current first.", true);
            return;
        }
        // Not a command: the current layer is session state, like the
        // selection, and is not on the undo stack.
        if (const auto status = document().setCurrentLayer(name); !status) {
            say(QString::fromStdString(status.error().describe()), true);
            return;
        }
        say("Current layer is now " + QString::fromStdString(name) + ".", false);
    });
}

LayerManagerDialog::LayerManagerDialog(katana::cad::Document& document, Log log, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>(this, document, std::move(log)))
{
    setWindowTitle("Layers");
    resize(980, 720);
    impl_->build();
    impl_->reload();
    // Registered last, once everything it reloads exists. Only what the
    // table shows moves it: the model, the library (which decides whether a
    // linetype is defined) and the current layer (shown in bold).
    impl_->watcher = std::make_unique<DocumentWatcher>(
        document, [this](const DocumentChanges& changes) {
            if (changes.model || changes.library || changes.current) {
                impl_->reload();
            }
        });
}

// The Impl goes first, taking the guard (every connection's receiver) and
// the watcher with it; the child widgets are deleted afterwards by ~QWidget.
LayerManagerDialog::~LayerManagerDialog() = default;

void LayerManagerDialog::showFirstRow()
{
    if (impl_->table->rowCount() > 0) {
        impl_->table->selectRow(0);
    }
}

std::string LayerManagerDialog::selectedLayer() const { return impl_->selectedLayer(); }

void LayerManagerDialog::selectLayer(const std::string& name) { impl_->selectRowOf(name); }

} // namespace katana::qt
