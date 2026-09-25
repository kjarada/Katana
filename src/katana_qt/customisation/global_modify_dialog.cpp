#include "customisation/global_modify_dialog.hpp"

#include <algorithm>
#include <string>
#include <utility>

#include <QCheckBox>
#include <QColor>
#include <QColorDialog>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPointer>
#include <QPushButton>
#include <QSignalBlocker>
#include <QTabWidget>
#include <QTimer>
#include <QVBoxLayout>

#include "customisation/document_watcher.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/model.hpp"

namespace katana::qt {

namespace {

using katana::cad::GlobalModify;
using katana::cad::ModifyFilter;
using katana::cad::ModifyScope;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Color;

constexpr const char* kByLayer = "ByLayer";

std::string text(const QString& value)
{
    return value.trimmed().toStdString();
}

// "#RRGGBB" or, when `byLayer` is allowed, ByLayer (an inner nullopt).
Result<std::optional<Color>> readColour(const QString& value, const char* field, bool byLayer)
{
    const std::string typed = text(value);
    if (byLayer && katana::core::equalsIgnoringCase(typed, kByLayer)) {
        return std::optional<Color>{};
    }
    auto colour = Color::fromHex(typed);
    if (!colour) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(field) + ": type #RRGGBB" + (byLayer ? " or ByLayer" : ""),
                         typed);
    }
    return std::optional<Color>(*colour);
}

QColor toQColor(const std::optional<Color>& colour)
{
    return colour ? QColor(colour->r, colour->g, colour->b) : QColor(Qt::white);
}

// "yes"/"no" as the Shown and Locked combos say it.
QComboBox* yesNo(QWidget* parent, const QString& name, const QString& yes, const QString& no)
{
    auto* box = new QComboBox(parent);
    box->setObjectName(name);
    box->addItem(yes);
    box->addItem(no);
    return box;
}

// Puts `names` in an editable combo, keeping what was typed or chosen.
void refill(QComboBox* box, const QStringList& names)
{
    const QSignalBlocker quiet(box);
    const QString kept = box->currentText();
    box->clear();
    box->addItems(names);
    box->setCurrentText(kept);
}

} // namespace

// ---- the form -------------------------------------------------------------------------------

struct GlobalModifyDialog::Impl {
    GlobalModifyDialog& dialog;
    CustomisationContext context;

    // Apply to, and Only those that match: the shared controls.
    ScopeFilterWidget* scopeFilter = nullptr;

    // A field of the Modify tabs: its tick and the editors it enables.
    struct Field {
        QCheckBox* set = nullptr;
        std::vector<QWidget*> editors;
    };
    std::vector<Field> fields;

    // Modify > Entities
    QCheckBox* setLayer = nullptr;
    QComboBox* layer = nullptr;
    QCheckBox* setColour = nullptr;
    QLineEdit* colour = nullptr;
    QCheckBox* setStyle = nullptr;
    QComboBox* style = nullptr;
    QCheckBox* setVisible = nullptr;
    QComboBox* visible = nullptr;
    QCheckBox* setSymbol = nullptr;
    QComboBox* symbol = nullptr;
    QDoubleSpinBox* symbolSize = nullptr;
    QCheckBox* setTextHeight = nullptr;
    QDoubleSpinBox* textHeight = nullptr;
    QCheckBox* setProperty = nullptr;
    QLineEdit* propertyKey = nullptr;
    QLineEdit* propertyValue = nullptr;
    QComboBox* propertyType = nullptr;
    QCheckBox* removeProperty = nullptr;
    QLineEdit* removeKey = nullptr;

    // Modify > Layers
    QCheckBox* setLayerColour = nullptr;
    QLineEdit* layerColour = nullptr;
    QCheckBox* setLayerLinetype = nullptr;
    QComboBox* layerLinetype = nullptr;
    QCheckBox* setLayerWeight = nullptr;
    QDoubleSpinBox* layerWeight = nullptr;
    QCheckBox* setLayerHatch = nullptr;
    QComboBox* layerHatch = nullptr;
    QCheckBox* setLayerDimStyle = nullptr;
    QComboBox* layerDimStyle = nullptr;
    QCheckBox* setLayerVisible = nullptr;
    QComboBox* layerVisible = nullptr;
    QCheckBox* setLayerLocked = nullptr;
    QComboBox* layerLocked = nullptr;

    // Modify > Styles
    QCheckBox* setStyleColour = nullptr;
    QLineEdit* styleColour = nullptr;
    QCheckBox* setStyleLinetype = nullptr;
    QComboBox* styleLinetype = nullptr;
    QCheckBox* setStyleWeight = nullptr;
    QDoubleSpinBox* styleWeight = nullptr;
    QCheckBox* setStyleHatch = nullptr;
    QComboBox* styleHatch = nullptr;
    QCheckBox* setStyleSymbol = nullptr;
    QComboBox* styleSymbol = nullptr;
    QCheckBox* setStyleSymbolSize = nullptr;
    QDoubleSpinBox* styleSymbolSize = nullptr;

    QTabWidget* tabs = nullptr;
    QLabel* summary = nullptr;
    QPushButton* previewButton = nullptr;
    QPushButton* selectButton = nullptr;
    QPushButton* applyButton = nullptr;
    QPushButton* closeButton = nullptr;
    QPointer<QColorDialog> colourDialog;
    // A preview after the edits of one turn of the event loop, not one per
    // keystroke.
    QTimer* previewSoon = nullptr;
    bool reloading = false;

    // Last, so it goes first: no delivery reaches a half-destroyed form.
    std::unique_ptr<DocumentWatcher> watcher;

    Impl(GlobalModifyDialog& owner, CustomisationContext given)
        : dialog(owner), context(std::move(given))
    {
    }

    [[nodiscard]] bool live() const { return watcher != nullptr && watcher->documentAlive(); }
    [[nodiscard]] katana::cad::Document& document() const { return *context.document; }

    void log(const QString& message, bool isError) const
    {
        if (context.log) {
            context.log(message, isError);
        }
    }

    void say(const QString& message, bool isError)
    {
        summary->setText(message);
        summary->setProperty("error", isError);
        summary->setStyleSheet(isError ? QStringLiteral("color: #c0392b;") : QString());
    }

    void schedulePreview()
    {
        if (!reloading) {
            previewSoon->start();
        }
    }

    // The tick in front of a field, enabling its editors.
    QCheckBox* tick(QWidget* parent, const QString& label, const QString& name,
                    std::vector<QWidget*> editors)
    {
        auto* box = new QCheckBox(label, parent);
        box->setObjectName(name);
        for (QWidget* editor : editors) {
            editor->setEnabled(false);
        }
        QObject::connect(box, &QCheckBox::toggled, &dialog, [this, editors](bool on) {
            for (QWidget* editor : editors) {
                editor->setEnabled(on);
            }
            schedulePreview();
        });
        fields.push_back(Field{box, std::move(editors)});
        return box;
    }

    QLineEdit* lineEdit(QWidget* parent, const QString& name, const QString& placeholder)
    {
        auto* edit = new QLineEdit(parent);
        edit->setObjectName(name);
        edit->setPlaceholderText(placeholder);
        QObject::connect(edit, &QLineEdit::textChanged, &dialog, [this] { schedulePreview(); });
        return edit;
    }

    QComboBox* editableCombo(QWidget* parent, const QString& name)
    {
        auto* box = new QComboBox(parent);
        box->setObjectName(name);
        box->setEditable(true);
        box->setInsertPolicy(QComboBox::NoInsert);
        QObject::connect(box, &QComboBox::currentTextChanged, &dialog,
                         [this] { schedulePreview(); });
        return box;
    }

    QDoubleSpinBox* spin(QWidget* parent, const QString& name, double value, int decimals,
                         double maximum)
    {
        auto* box = new QDoubleSpinBox(parent);
        box->setObjectName(name);
        box->setDecimals(decimals);
        box->setRange(0.0, maximum);
        box->setValue(value);
        QObject::connect(box, &QDoubleSpinBox::valueChanged, &dialog,
                         [this] { schedulePreview(); });
        return box;
    }

    // A colour typed as text, with a Choose... button beside it opening
    // Qt's colour dialog with open(), never exec().
    QWidget* colourRow(QWidget* parent, QLineEdit*& edit, const QString& name, bool byLayer)
    {
        auto* row = new QWidget(parent);
        auto* layout = new QHBoxLayout(row);
        layout->setContentsMargins(0, 0, 0, 0);
        edit = lineEdit(row, name,
                        byLayer ? QStringLiteral("#RRGGBB or ByLayer") : QStringLiteral("#RRGGBB"));
        auto* choose = new QPushButton(QStringLiteral("Choose..."), row);
        choose->setObjectName(name + QStringLiteral("Choose"));
        layout->addWidget(edit, 1);
        layout->addWidget(choose);
        QLineEdit* target = edit;
        QObject::connect(choose, &QPushButton::clicked, &dialog, [this, target] {
            if (colourDialog.isNull()) {
                colourDialog = new QColorDialog(&dialog);
                colourDialog->setObjectName(QStringLiteral("globalModifyColourDialog"));
                colourDialog->setOption(QColorDialog::DontUseNativeDialog);
            }
            QObject::disconnect(colourDialog, &QColorDialog::colorSelected, nullptr, nullptr);
            QObject::connect(colourDialog, &QColorDialog::colorSelected, target,
                             [target](const QColor& picked) {
                                 if (picked.isValid()) {
                                     target->setText(picked.name().toUpper());
                                 }
                             });
            const auto current = readColour(target->text(), "colour", true);
            colourDialog->setCurrentColor(current ? toQColor(*current) : QColor(Qt::white));
            colourDialog->open();
        });
        return row;
    }

    void build();
    QWidget* buildEntitiesTab(QWidget* parent);
    QWidget* buildLayersTab(QWidget* parent);
    QWidget* buildStylesTab(QWidget* parent);
    void reload();
};

void GlobalModifyDialog::Impl::build()
{
    dialog.setWindowTitle(QStringLiteral("Global Modify"));
    // First: a control set up below (the scope's default radio button)
    // signals as it is checked, and every signal schedules a preview.
    previewSoon = new QTimer(&dialog);
    previewSoon->setSingleShot(true);
    previewSoon->setInterval(150);
    QObject::connect(previewSoon, &QTimer::timeout, &dialog, [this] {
        if (dialog.isVisible()) {
            dialog.preview();
        }
    });
    auto* outer = new QVBoxLayout(&dialog);

    auto* columns = new QHBoxLayout();
    auto* left = new QVBoxLayout();
    scopeFilter = new ScopeFilterWidget(QStringLiteral("globalModify"), &dialog);
    // The dialog's own views function, read afresh each time it is needed.
    scopeFilter->views = [this] {
        return dialog.views ? dialog.views() : ScopeFilterWidget::noWorkspaceViews();
    };
    scopeFilter->onChanged = [this] { schedulePreview(); };
    left->addWidget(scopeFilter);
    left->addStretch(1);
    columns->addLayout(left, 2);

    auto* changeBox = new QGroupBox(QStringLiteral("Modify"), &dialog);
    changeBox->setObjectName(QStringLiteral("globalModifyFieldsGroup"));
    auto* changeLayout = new QVBoxLayout(changeBox);
    auto* hint = new QLabel(QStringLiteral("Tick a field to change it; an unticked field is "
                                           "left as each one has it."),
                            changeBox);
    hint->setWordWrap(true);
    changeLayout->addWidget(hint);
    tabs = new QTabWidget(changeBox);
    tabs->setObjectName(QStringLiteral("globalModifyTabs"));
    tabs->addTab(buildEntitiesTab(tabs), QStringLiteral("Entities"));
    tabs->addTab(buildLayersTab(tabs), QStringLiteral("Their Layers"));
    tabs->addTab(buildStylesTab(tabs), QStringLiteral("Their Styles"));
    changeLayout->addWidget(tabs);
    columns->addWidget(changeBox, 3);
    outer->addLayout(columns, 1);

    summary = new QLabel(&dialog);
    summary->setObjectName(QStringLiteral("globalModifySummary"));
    summary->setWordWrap(true);
    summary->setTextInteractionFlags(Qt::TextSelectableByMouse);
    summary->setMinimumHeight(summary->fontMetrics().height() * 3);
    outer->addWidget(summary);

    auto* buttons = new QHBoxLayout();
    const auto button = [&](const QString& label, const QString& name, const QString& tip) {
        auto* push = new QPushButton(label, &dialog);
        push->setObjectName(name);
        push->setToolTip(tip);
        push->setAutoDefault(false);
        buttons->addWidget(push);
        return push;
    };
    previewButton = button(QStringLiteral("Preview"), QStringLiteral("globalModifyPreview"),
                           QStringLiteral("Say what Apply would change, changing nothing"));
    selectButton = button(QStringLiteral("Select Matches"), QStringLiteral("globalModifySelect"),
                          QStringLiteral("Select what the scope and the filter take, and show it"));
    buttons->addStretch(1);
    applyButton = button(QStringLiteral("Apply"), QStringLiteral("globalModifyApply"),
                         QStringLiteral("Make the change: one Undo puts everything back"));
    closeButton = button(QStringLiteral("Close"), QStringLiteral("globalModifyClose"), QString());
    outer->addLayout(buttons);

    QObject::connect(previewButton, &QPushButton::clicked, &dialog, [this] { dialog.preview(); });
    QObject::connect(selectButton, &QPushButton::clicked, &dialog,
                     [this] { dialog.selectMatches(); });
    QObject::connect(applyButton, &QPushButton::clicked, &dialog, [this] { dialog.apply(); });
    QObject::connect(closeButton, &QPushButton::clicked, &dialog, [this] { dialog.close(); });
}

QWidget* GlobalModifyDialog::Impl::buildEntitiesTab(QWidget* parent)
{
    auto* page = new QWidget(parent);
    page->setObjectName(QStringLiteral("globalModifyEntitiesTab"));
    auto* grid = new QGridLayout(page);
    int row = 0;
    const auto add = [&](QCheckBox* box, QWidget* editor) {
        grid->addWidget(box, row, 0);
        grid->addWidget(editor, row, 1);
        ++row;
    };

    layer = editableCombo(page, QStringLiteral("globalModifyLayer"));
    layer->setToolTip(QStringLiteral("Moves them onto this layer; a new name makes the layer"));
    setLayer = tick(page, QStringLiteral("Layer"), QStringLiteral("globalModifySetLayer"), {layer});
    add(setLayer, layer);

    QWidget* colourEditor = colourRow(page, colour, QStringLiteral("globalModifyColour"), true);
    setColour = tick(page, QStringLiteral("Colour"), QStringLiteral("globalModifySetColour"),
                     {colourEditor});
    add(setColour, colourEditor);

    style = editableCombo(page, QStringLiteral("globalModifyStyle"));
    setStyle = tick(page, QStringLiteral("Style"), QStringLiteral("globalModifySetStyle"), {style});
    add(setStyle, style);

    visible = yesNo(page, QStringLiteral("globalModifyVisible"), QStringLiteral("Shown"),
                    QStringLiteral("Hidden"));
    QObject::connect(visible, &QComboBox::currentIndexChanged, &dialog,
                     [this] { schedulePreview(); });
    setVisible =
        tick(page, QStringLiteral("Shown"), QStringLiteral("globalModifySetVisible"), {visible});
    add(setVisible, visible);

    auto* symbolRow = new QWidget(page);
    auto* symbolLayout = new QHBoxLayout(symbolRow);
    symbolLayout->setContentsMargins(0, 0, 0, 0);
    symbol = editableCombo(symbolRow, QStringLiteral("globalModifySymbol"));
    symbolSize = spin(symbolRow, QStringLiteral("globalModifySymbolSize"), 0.0, 3, 1e6);
    symbolSize->setToolTip(QStringLiteral("Width in model units; 0 is the symbol's own size"));
    symbolSize->setSpecialValueText(QStringLiteral("own size"));
    symbolLayout->addWidget(symbol, 1);
    symbolLayout->addWidget(symbolSize);
    setSymbol = tick(page, QStringLiteral("Symbol (points)"),
                     QStringLiteral("globalModifySetSymbol"), {symbolRow});
    setSymbol->setToolTip(QStringLiteral("Puts the points in a style that draws this symbol, "
                                         "found or made"));
    add(setSymbol, symbolRow);

    textHeight = spin(page, QStringLiteral("globalModifyTextHeight"), 2.5, 3, 1e6);
    setTextHeight = tick(page, QStringLiteral("Text height"),
                         QStringLiteral("globalModifySetTextHeight"), {textHeight});
    add(setTextHeight, textHeight);

    auto* propertyRow = new QWidget(page);
    auto* propertyLayout = new QHBoxLayout(propertyRow);
    propertyLayout->setContentsMargins(0, 0, 0, 0);
    propertyKey =
        lineEdit(propertyRow, QStringLiteral("globalModifyPropertyKey"), QStringLiteral("key"));
    propertyValue =
        lineEdit(propertyRow, QStringLiteral("globalModifyPropertyValue"), QStringLiteral("value"));
    propertyType = new QComboBox(propertyRow);
    propertyType->setObjectName(QStringLiteral("globalModifyPropertyType"));
    propertyType->addItems({QStringLiteral("text"), QStringLiteral("integer"),
                            QStringLiteral("real"), QStringLiteral("boolean")});
    QObject::connect(propertyType, &QComboBox::currentIndexChanged, &dialog,
                     [this] { schedulePreview(); });
    propertyLayout->addWidget(propertyKey);
    propertyLayout->addWidget(propertyValue);
    propertyLayout->addWidget(propertyType);
    setProperty = tick(page, QStringLiteral("Set property"),
                       QStringLiteral("globalModifySetProperty"), {propertyRow});
    add(setProperty, propertyRow);

    removeKey = lineEdit(page, QStringLiteral("globalModifyRemoveKey"), QStringLiteral("key"));
    removeProperty = tick(page, QStringLiteral("Remove property"),
                          QStringLiteral("globalModifyRemoveProperty"), {removeKey});
    add(removeProperty, removeKey);

    grid->setRowStretch(row, 1);
    grid->setColumnStretch(1, 1);
    return page;
}

QWidget* GlobalModifyDialog::Impl::buildLayersTab(QWidget* parent)
{
    auto* page = new QWidget(parent);
    page->setObjectName(QStringLiteral("globalModifyLayersTab"));
    auto* grid = new QGridLayout(page);
    int row = 0;
    auto* note = new QLabel(QStringLiteral("The layers the matched entities are on - or, when "
                                           "applying to layers, the checked layers themselves."),
                            page);
    note->setWordWrap(true);
    grid->addWidget(note, row++, 0, 1, 2);
    const auto add = [&](QCheckBox* box, QWidget* editor) {
        grid->addWidget(box, row, 0);
        grid->addWidget(editor, row, 1);
        ++row;
    };

    QWidget* colourEditor =
        colourRow(page, layerColour, QStringLiteral("globalModifyLayerColour"), false);
    setLayerColour = tick(page, QStringLiteral("Colour"),
                          QStringLiteral("globalModifySetLayerColour"), {colourEditor});
    add(setLayerColour, colourEditor);

    layerLinetype = editableCombo(page, QStringLiteral("globalModifyLayerLinetype"));
    setLayerLinetype = tick(page, QStringLiteral("Linetype"),
                            QStringLiteral("globalModifySetLayerLinetype"), {layerLinetype});
    add(setLayerLinetype, layerLinetype);

    layerWeight = spin(page, QStringLiteral("globalModifyLayerWeight"), 0.25, 2, 10.0);
    layerWeight->setSuffix(QStringLiteral(" mm"));
    setLayerWeight = tick(page, QStringLiteral("Line weight"),
                          QStringLiteral("globalModifySetLayerWeight"), {layerWeight});
    add(setLayerWeight, layerWeight);

    layerHatch = editableCombo(page, QStringLiteral("globalModifyLayerHatch"));
    setLayerHatch = tick(page, QStringLiteral("Hatch"), QStringLiteral("globalModifySetLayerHatch"),
                         {layerHatch});
    add(setLayerHatch, layerHatch);

    layerDimStyle = editableCombo(page, QStringLiteral("globalModifyLayerDimStyle"));
    setLayerDimStyle = tick(page, QStringLiteral("Dimension style"),
                            QStringLiteral("globalModifySetLayerDimStyle"), {layerDimStyle});
    add(setLayerDimStyle, layerDimStyle);

    layerVisible = yesNo(page, QStringLiteral("globalModifyLayerVisible"), QStringLiteral("Shown"),
                         QStringLiteral("Hidden"));
    layerLocked = yesNo(page, QStringLiteral("globalModifyLayerLocked"), QStringLiteral("Locked"),
                        QStringLiteral("Unlocked"));
    for (QComboBox* box : {layerVisible, layerLocked}) {
        QObject::connect(box, &QComboBox::currentIndexChanged, &dialog,
                         [this] { schedulePreview(); });
    }
    setLayerVisible = tick(page, QStringLiteral("Shown"),
                           QStringLiteral("globalModifySetLayerVisible"), {layerVisible});
    add(setLayerVisible, layerVisible);
    setLayerLocked = tick(page, QStringLiteral("Locked"),
                          QStringLiteral("globalModifySetLayerLocked"), {layerLocked});
    setLayerLocked->setToolTip(QStringLiteral("An unlock comes first, so what is on the layer "
                                              "can be changed; a lock comes last"));
    add(setLayerLocked, layerLocked);

    grid->setRowStretch(row, 1);
    grid->setColumnStretch(1, 1);
    return page;
}

QWidget* GlobalModifyDialog::Impl::buildStylesTab(QWidget* parent)
{
    auto* page = new QWidget(parent);
    page->setObjectName(QStringLiteral("globalModifyStylesTab"));
    auto* grid = new QGridLayout(page);
    int row = 0;
    auto* note = new QLabel(QStringLiteral("The styles the matched entities wear. A style is "
                                           "shared: everything wearing it is redrawn, in the "
                                           "scope or not, and the summary counts those."),
                            page);
    note->setWordWrap(true);
    grid->addWidget(note, row++, 0, 1, 2);
    const auto add = [&](QCheckBox* box, QWidget* editor) {
        grid->addWidget(box, row, 0);
        grid->addWidget(editor, row, 1);
        ++row;
    };

    QWidget* colourEditor =
        colourRow(page, styleColour, QStringLiteral("globalModifyStyleColour"), true);
    setStyleColour = tick(page, QStringLiteral("Colour"),
                          QStringLiteral("globalModifySetStyleColour"), {colourEditor});
    add(setStyleColour, colourEditor);

    styleLinetype = editableCombo(page, QStringLiteral("globalModifyStyleLinetype"));
    setStyleLinetype = tick(page, QStringLiteral("Linetype"),
                            QStringLiteral("globalModifySetStyleLinetype"), {styleLinetype});
    add(setStyleLinetype, styleLinetype);

    styleWeight = spin(page, QStringLiteral("globalModifyStyleWeight"), 0.25, 2, 10.0);
    styleWeight->setSuffix(QStringLiteral(" mm"));
    setStyleWeight = tick(page, QStringLiteral("Line weight"),
                          QStringLiteral("globalModifySetStyleWeight"), {styleWeight});
    add(setStyleWeight, styleWeight);

    styleHatch = editableCombo(page, QStringLiteral("globalModifyStyleHatch"));
    setStyleHatch = tick(page, QStringLiteral("Hatch"), QStringLiteral("globalModifySetStyleHatch"),
                         {styleHatch});
    add(setStyleHatch, styleHatch);

    styleSymbol = editableCombo(page, QStringLiteral("globalModifyStyleSymbol"));
    setStyleSymbol = tick(page, QStringLiteral("Symbol"),
                          QStringLiteral("globalModifySetStyleSymbol"), {styleSymbol});
    add(setStyleSymbol, styleSymbol);

    styleSymbolSize = spin(page, QStringLiteral("globalModifyStyleSymbolSize"), 0.0, 3, 1e6);
    styleSymbolSize->setSpecialValueText(QStringLiteral("own size"));
    setStyleSymbolSize = tick(page, QStringLiteral("Symbol size"),
                              QStringLiteral("globalModifySetStyleSymbolSize"), {styleSymbolSize});
    add(setStyleSymbolSize, styleSymbolSize);

    grid->setRowStretch(row, 1);
    grid->setColumnStretch(1, 1);
    return page;
}

void GlobalModifyDialog::Impl::reload()
{
    if (!live()) {
        return;
    }
    reloading = true;
    const katana::entity::Model& model = document().model();

    // The layers and the views, keeping what is ticked and chosen.
    scopeFilter->reload(model);

    QStringList layerNames;
    for (const std::string& name : model.layers.names()) {
        layerNames << QString::fromStdString(name);
    }
    refill(layer, layerNames);

    QStringList styleNames{QString::fromLatin1(kByLayer)};
    for (const std::string& name : model.styles.names()) {
        styleNames << QString::fromStdString(name);
    }
    refill(style, styleNames);

    QStringList symbols;
    for (const katana::cad::CatalogueEntry& entry : katana::cad::symbolChoices(document())) {
        symbols << QString::fromStdString(entry.name);
    }
    refill(symbol, symbols);
    QStringList styleSymbols = symbols;
    styleSymbols.prepend(QStringLiteral("(plain mark)"));
    refill(styleSymbol, styleSymbols);

    QStringList layerLinetypes;
    for (const katana::cad::CatalogueEntry& entry :
         katana::cad::linetypeChoices(document(), false)) {
        layerLinetypes << QString::fromStdString(entry.name);
    }
    refill(layerLinetype, layerLinetypes);
    QStringList styleLinetypes;
    for (const katana::cad::CatalogueEntry& entry :
         katana::cad::linetypeChoices(document(), true)) {
        styleLinetypes << QString::fromStdString(entry.name);
    }
    refill(styleLinetype, styleLinetypes);

    QStringList hatches;
    for (const std::string& name : model.hatchPatterns.names()) {
        hatches << QString::fromStdString(name);
    }
    refill(layerHatch, hatches);
    hatches.prepend(QString::fromLatin1(kByLayer));
    refill(styleHatch, hatches);

    QStringList dimStyles{QStringLiteral("(document default)")};
    for (const std::string& name : model.dimensionStyles.names()) {
        dimStyles << QString::fromStdString(name);
    }
    refill(layerDimStyle, dimStyles);

    reloading = false;
}

// ---- the dialog -----------------------------------------------------------------------------

GlobalModifyDialog::GlobalModifyDialog(const CustomisationContext& context, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>(*this, context))
{
    setObjectName(QStringLiteral("globalModifyDialog"));
    impl_->build();
    impl_->watcher = std::make_unique<DocumentWatcher>(
        *context.document, [this](const DocumentChanges& changes) {
            if (changes.model || changes.library) {
                reload();
            }
            if ((changes.model || changes.selection) && isVisible()) {
                // What matches, and what it would change, may differ now.
                preview();
            }
        });
    reload();
    impl_->say(QStringLiteral("Choose what to apply to, tick what to change, then Preview or "
                              "Apply."),
               false);
}

GlobalModifyDialog::~GlobalModifyDialog() = default;

void GlobalModifyDialog::reload()
{
    impl_->reload();
}

QString GlobalModifyDialog::summaryText() const
{
    return impl_->summary->text();
}

Result<ModifyScope> GlobalModifyDialog::scope() const
{
    return impl_->scopeFilter->scope();
}

Result<ModifyFilter> GlobalModifyDialog::filter() const
{
    return impl_->scopeFilter->filter();
}

Result<GlobalModify> GlobalModifyDialog::modification() const
{
    const Impl& d = *impl_;
    GlobalModify change;
    katana::cad::EntityModify& entities = change.entities;
    if (d.setLayer->isChecked()) {
        entities.layer = text(d.layer->currentText());
    }
    if (d.setColour->isChecked()) {
        auto colour = readColour(d.colour->text(), "the colour", true);
        if (!colour) {
            return colour.error();
        }
        entities.colour = *colour;
    }
    if (d.setStyle->isChecked()) {
        const std::string name = text(d.style->currentText());
        entities.style = katana::core::equalsIgnoringCase(name, kByLayer) ? std::string() : name;
    }
    if (d.setVisible->isChecked()) {
        entities.visible = d.visible->currentIndex() == 0;
    }
    if (d.setSymbol->isChecked()) {
        entities.symbol =
            katana::cad::SymbolModify{text(d.symbol->currentText()), d.symbolSize->value()};
    }
    if (d.setTextHeight->isChecked()) {
        entities.textHeight = d.textHeight->value();
    }
    if (d.setProperty->isChecked()) {
        const std::string key = text(d.propertyKey->text());
        const std::string value = d.propertyValue->text().toStdString();
        katana::entity::PropertyValue typed = value;
        switch (d.propertyType->currentIndex()) {
        case 1: {
            const auto integer = katana::core::parseInteger(katana::core::trimmed(value));
            if (!integer) {
                return makeError(ErrorCode::InvalidArgument, "the property's value: not an integer",
                                 value);
            }
            typed = *integer;
            break;
        }
        case 2: {
            const auto real = katana::core::parseFiniteDouble(katana::core::trimmed(value));
            if (!real) {
                return makeError(ErrorCode::InvalidArgument, "the property's value: not a number",
                                 value);
            }
            typed = *real;
            break;
        }
        case 3: {
            const std::string folded = katana::core::lowered(katana::core::trimmed(value));
            if (folded != "true" && folded != "false") {
                return makeError(ErrorCode::InvalidArgument, "the property's value: true or false",
                                 value);
            }
            typed = folded == "true";
            break;
        }
        default:
            break;
        }
        entities.setProperties.emplace_back(key, std::move(typed));
    }
    if (d.removeProperty->isChecked()) {
        entities.removeProperties.push_back(text(d.removeKey->text()));
    }

    katana::cad::LayerModify& layers = change.layers;
    if (d.setLayerColour->isChecked()) {
        auto colour = readColour(d.layerColour->text(), "the layers' colour", false);
        if (!colour) {
            return colour.error();
        }
        layers.colour = **colour;
    }
    if (d.setLayerLinetype->isChecked()) {
        layers.linetype = text(d.layerLinetype->currentText());
    }
    if (d.setLayerWeight->isChecked()) {
        layers.lineWeight = d.layerWeight->value();
    }
    if (d.setLayerHatch->isChecked()) {
        layers.hatchPattern = text(d.layerHatch->currentText());
    }
    if (d.setLayerDimStyle->isChecked()) {
        layers.dimensionStyle =
            d.layerDimStyle->currentIndex() == 0 &&
                    d.layerDimStyle->currentText() == d.layerDimStyle->itemText(0)
                ? std::string()
                : text(d.layerDimStyle->currentText());
    }
    if (d.setLayerVisible->isChecked()) {
        layers.visible = d.layerVisible->currentIndex() == 0;
    }
    if (d.setLayerLocked->isChecked()) {
        layers.locked = d.layerLocked->currentIndex() == 0;
    }

    katana::cad::StyleModify& styles = change.styles;
    if (d.setStyleColour->isChecked()) {
        auto colour = readColour(d.styleColour->text(), "the styles' colour", true);
        if (!colour) {
            return colour.error();
        }
        styles.colour = *colour;
    }
    if (d.setStyleLinetype->isChecked()) {
        styles.linetype = text(d.styleLinetype->currentText());
    }
    if (d.setStyleWeight->isChecked()) {
        styles.lineWeight = d.styleWeight->value();
    }
    if (d.setStyleHatch->isChecked()) {
        const std::string name = text(d.styleHatch->currentText());
        styles.hatchPattern =
            katana::core::equalsIgnoringCase(name, kByLayer) ? std::string() : name;
    }
    if (d.setStyleSymbol->isChecked()) {
        const QString shown = d.styleSymbol->currentText();
        styles.symbol = shown == d.styleSymbol->itemText(0) ? std::string() : text(shown);
    }
    if (d.setStyleSymbolSize->isChecked()) {
        styles.symbolSize = d.styleSymbolSize->value();
    }
    return change;
}

namespace {

struct Request {
    ModifyScope scope;
    ModifyFilter filter;
    GlobalModify change;
};

// The three parts of the form, or the first that does not read.
Result<Request> readRequest(const GlobalModifyDialog& dialog)
{
    auto scope = dialog.scope();
    if (!scope) {
        return scope.error();
    }
    auto filter = dialog.filter();
    if (!filter) {
        return filter.error();
    }
    auto change = dialog.modification();
    if (!change) {
        return change.error();
    }
    return Request{std::move(*scope), std::move(*filter), std::move(*change)};
}

QString matchCount(std::size_t count, const char* one, const char* many)
{
    return QString::number(count) + QString::fromLatin1(count == 1 ? one : many);
}

} // namespace

bool GlobalModifyDialog::preview()
{
    Impl& d = *impl_;
    if (!d.live()) {
        return false;
    }
    auto request = readRequest(*this);
    if (!request) {
        d.say(QString::fromStdString(request.error().describe()), true);
        return false;
    }
    if (request->change.empty()) {
        auto matched = katana::cad::matchEntities(d.document(), request->scope, request->filter);
        if (!matched) {
            d.say(QString::fromStdString(matched.error().describe()), true);
            return false;
        }
        d.say(matchCount(matched->size(), " entity matches", " entities match") +
                  QStringLiteral(". Tick a field to change."),
              false);
        return true;
    }
    auto plan = katana::cad::planGlobalModify(d.document(), request->scope, request->filter,
                                              request->change);
    if (!plan) {
        d.say(QString::fromStdString(plan.error().describe()), true);
        return false;
    }
    d.say(QString::fromStdString(plan->summary()), false);
    return true;
}

bool GlobalModifyDialog::apply()
{
    Impl& d = *impl_;
    if (!d.live()) {
        return false;
    }
    d.previewSoon->stop();
    const auto refuse = [&d](const katana::core::Error& error) {
        const QString message = QString::fromStdString(error.describe());
        d.say(message, true);
        d.log(QStringLiteral("Global Modify: ") + message, true);
        return false;
    };
    auto request = readRequest(*this);
    if (!request) {
        return refuse(request.error());
    }
    auto plan = katana::cad::planGlobalModify(d.document(), request->scope, request->filter,
                                              request->change);
    if (!plan) {
        return refuse(plan.error());
    }
    const QString summary = QString::fromStdString(plan->summary());
    if (plan->command == nullptr) {
        d.say(summary, false);
        d.log(QStringLiteral("Global Modify: ") + summary, false);
        return false;
    }
    if (const auto status = d.document().execute(std::move(plan->command)); !status) {
        return refuse(status.error());
    }
    d.say(QStringLiteral("Done: ") + summary + QStringLiteral(" One Undo puts it all back."),
          false);
    d.log(QStringLiteral("Global Modify: ") + summary + QStringLiteral(" (one Undo restores it)"),
          false);
    return true;
}

bool GlobalModifyDialog::selectMatches()
{
    Impl& d = *impl_;
    if (!d.live()) {
        return false;
    }
    auto scope = this->scope();
    auto filter = this->filter();
    if (!scope || !filter) {
        d.say(QString::fromStdString((!scope ? scope.error() : filter.error()).describe()), true);
        return false;
    }
    auto matched = katana::cad::matchEntities(d.document(), *scope, *filter);
    if (!matched) {
        d.say(QString::fromStdString(matched.error().describe()), true);
        return false;
    }
    const std::size_t count = matched->size();
    if (d.context.selectAndShow) {
        d.context.selectAndShow(*matched);
    } else {
        d.document().selection().set(std::move(*matched));
        d.document().notifySelectionChanged();
    }
    const QString said = matchCount(count, " entity selected.", " entities selected.");
    d.say(said, false);
    d.log(QStringLiteral("Global Modify: ") + said, false);
    return count > 0;
}

} // namespace katana::qt
