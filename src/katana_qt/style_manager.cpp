#include "style_manager.hpp"

#include <algorithm>
#include <memory>
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
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPushButton>
#include <QRegularExpression>
#include <QTabWidget>
#include <QTableWidget>
#include <QVBoxLayout>

#include "katana/cad/dashing.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/symbols.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

namespace cmd = katana::commands;

namespace katana::qt {

namespace {

using katana::entity::Color;
using katana::entity::Linetype;
using katana::entity::Style;

constexpr const char* kByLayer = "(ByLayer)";
constexpr const char* kNoSymbolLabel = "(none)";

QColor toQColor(const Color& color)
{
    return QColor(color.r, color.g, color.b, color.a);
}

Color fromQColor(const QColor& color)
{
    return Color{static_cast<std::uint8_t>(color.red()), static_cast<std::uint8_t>(color.green()),
                 static_cast<std::uint8_t>(color.blue()),
                 static_cast<std::uint8_t>(color.alpha())};
}

QString patternText(const Linetype& linetype)
{
    QString text;
    for (const auto& element : linetype.pattern) {
        text += (text.isEmpty() ? "" : " ") + QString::number(element.length, 'g', 6);
    }
    return text.isEmpty() ? QString("(solid)") : text;
}

// A sample of what a style draws: a line in its linetype, weight and colour,
// with its symbol at the middle. The same two functions the viewport and the
// plot use (cad::qtDashPattern, cad::symbolStrokes), so a preview that looks
// right is evidence about the drawing and not a second opinion.
class StylePreview : public QWidget {
  public:
    explicit StylePreview(QWidget* parent) : QWidget(parent)
    {
        setMinimumHeight(56);
        setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
    }

    void setSample(const Linetype& linetype, const Style& style, const QColor& fallback)
    {
        linetype_ = linetype;
        style_ = style;
        fallback_ = fallback;
        update();
    }

  protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing, true);
        painter.fillRect(rect(), palette().base());

        const QColor colour = style_.color ? toQColor(*style_.color) : fallback_;
        // The preview is a plot at a stated scale rather than "some pixels":
        // 20 pixels to the model unit, so a 1 m dash reads as 20 px and a
        // 1.5 m symbol fills the height.
        constexpr double kPixelsPerModelUnit = 20.0;
        const double penWidth = std::max(1.0, style_.lineWeight * 4.0);
        QPen pen(colour, penWidth);
        pen.setCapStyle(Qt::FlatCap);
        katana::cad::DashOptions options;
        options.viewScale = kPixelsPerModelUnit;
        const auto dashes = katana::cad::qtDashPattern(linetype_, options, penWidth);
        if (!dashes.empty()) {
            pen.setDashPattern(QList<qreal>(dashes.begin(), dashes.end()));
        }
        painter.setPen(pen);
        const double y = height() / 2.0;
        painter.drawLine(QPointF(8.0, y), QPointF(width() - 8.0, y));

        if (style_.symbol.empty()) {
            return;
        }
        // Drawn solid: a symbol is not dashed, whatever the style's linetype.
        painter.setPen(QPen(colour, penWidth));
        const double size = style_.symbolSize > 0.0 ? style_.symbolSize : 0.6;
        const double half = 0.5 * size * kPixelsPerModelUnit;
        const double cx = width() / 2.0;
        for (const auto& stroke :
             katana::cad::symbolStrokes(style_.symbol, katana::geometry::Point2(0.0, 0.0),
                                        std::min(half, height() / 2.0 - 4.0))) {
            QPolygonF polygon;
            for (const auto& vertex : stroke.vertices) {
                polygon << QPointF(cx + vertex.x, y - vertex.y);
            }
            if (stroke.closed && !stroke.vertices.empty()) {
                polygon << QPointF(cx + stroke.vertices.front().x, y - stroke.vertices.front().y);
            }
            painter.drawPolyline(polygon);
        }
    }

  private:
    Linetype linetype_{};
    Style style_{};
    QColor fallback_{Qt::white};
};

} // namespace

struct StyleManagerDialog::Impl {
    katana::cad::Document& document;
    Log log;

    QTableWidget* styleTable = nullptr;
    QComboBox* linetypeBox = nullptr;
    QDoubleSpinBox* weightBox = nullptr;
    QPushButton* colourButton = nullptr;
    QCheckBox* byLayerBox = nullptr;
    QComboBox* hatchBox = nullptr;
    QComboBox* symbolBox = nullptr;
    QDoubleSpinBox* symbolSizeBox = nullptr;
    QLineEdit* descriptionBox = nullptr;
    StylePreview* stylePreview = nullptr;
    std::optional<Color> pickedColour{};

    QTableWidget* linetypeTable = nullptr;
    QLineEdit* patternBox = nullptr;
    QLineEdit* linetypeDescriptionBox = nullptr;
    StylePreview* linetypePreview = nullptr;

    Impl(katana::cad::Document& d, Log l) : document(d), log(std::move(l)) {}

    [[nodiscard]] std::string selectedStyle() const { return selectedName(styleTable); }
    [[nodiscard]] std::string selectedLinetype() const { return selectedName(linetypeTable); }

    static std::string selectedName(const QTableWidget* table)
    {
        if (table == nullptr) {
            return {};
        }
        const int row = table->currentRow();
        if (row < 0 || table->item(row, 0) == nullptr) {
            return {};
        }
        return table->item(row, 0)->text().toStdString();
    }

    // Reports what a command did, and says nothing on success beyond the
    // sentence given: the drawing itself is the other half of the answer.
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

StyleManagerDialog::StyleManagerDialog(katana::cad::Document& document, Log log, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>(document, std::move(log)))
{
    setWindowTitle("Styles and Linetypes");
    resize(900, 720);

    auto* tabs = new QTabWidget(this);

    // ---- styles ------------------------------------------------------------
    auto* stylePage = new QWidget(tabs);
    auto* styleLayout = new QVBoxLayout(stylePage);
    impl_->styleTable = new QTableWidget(stylePage);
    impl_->styleTable->setColumnCount(7);
    impl_->styleTable->setHorizontalHeaderLabels(
        {"Name", "Linetype", "Weight", "Colour", "Hatch", "Symbol", "Description"});
    // Read-only, and edited through the form below. A table that edits in
    // place would run a command from inside its own itemChanged signal, which
    // is the shape of the layer-panel crash recorded in docs/cad.md.
    impl_->styleTable->setEditTriggers(QAbstractItemView::NoEditTriggers);
    impl_->styleTable->setSelectionBehavior(QAbstractItemView::SelectRows);
    impl_->styleTable->setSelectionMode(QAbstractItemView::SingleSelection);
    impl_->styleTable->horizontalHeader()->setStretchLastSection(true);
    styleLayout->addWidget(impl_->styleTable, 1);

    auto* editor = new QGroupBox("Definition", stylePage);
    // The table is the subject and takes the room; the form is as tall as
    // its rows. A 12d import brings hundreds of styles and a list four rows
    // deep is not a list.
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
    auto* colourRow = new QWidget(editor);
    auto* colourLayout = new QHBoxLayout(colourRow);
    colourLayout->setContentsMargins(0, 0, 0, 0);
    impl_->colourButton = new QPushButton("Choose...", colourRow);
    impl_->byLayerBox = new QCheckBox("ByLayer", colourRow);
    colourLayout->addWidget(impl_->colourButton);
    colourLayout->addWidget(impl_->byLayerBox);
    colourLayout->addStretch(1);
    form->addRow("Colour", colourRow);
    impl_->hatchBox = new QComboBox(editor);
    form->addRow("Hatch", impl_->hatchBox);
    impl_->symbolBox = new QComboBox(editor);
    form->addRow("Symbol (points)", impl_->symbolBox);
    impl_->symbolSizeBox = new QDoubleSpinBox(editor);
    impl_->symbolSizeBox->setRange(0.0, 10000.0);
    impl_->symbolSizeBox->setDecimals(3);
    impl_->symbolSizeBox->setSuffix(" model units wide (0 = the plain mark)");
    form->addRow("Symbol size", impl_->symbolSizeBox);
    impl_->descriptionBox = new QLineEdit(editor);
    form->addRow("Description", impl_->descriptionBox);
    impl_->stylePreview = new StylePreview(editor);
    form->addRow("Preview", impl_->stylePreview);
    styleLayout->addWidget(editor);

    auto* styleButtons = new QWidget(stylePage);
    auto* styleButtonLayout = new QHBoxLayout(styleButtons);
    styleButtonLayout->setContentsMargins(0, 0, 0, 0);
    auto* newStyle = new QPushButton("New...", styleButtons);
    auto* renameStyle = new QPushButton("Rename...", styleButtons);
    auto* deleteStyle = new QPushButton("Delete", styleButtons);
    auto* applyStyle = new QPushButton("Save Changes", styleButtons);
    auto* applyToSelection = new QPushButton("Apply to Selection", styleButtons);
    // Said out loud because the dialog is modal: the selection is whatever
    // was picked in the drawing before it opened.
    applyToSelection->setToolTip("Give the entities selected before this dialog opened this style");
    renameStyle->setToolTip("Rename it; every entity wearing it comes too, as one undo step");
    deleteStyle->setToolTip("Refused while any entity still wears it, and says which");
    applyStyle->setToolTip("Store the definition above as this style");
    for (QPushButton* button :
         {newStyle, renameStyle, deleteStyle, applyStyle, applyToSelection}) {
        styleButtonLayout->addWidget(button);
    }
    styleButtonLayout->addStretch(1);
    styleLayout->addWidget(styleButtons);
    tabs->addTab(stylePage, "Styles");

    // ---- linetypes ---------------------------------------------------------
    auto* linetypePage = new QWidget(tabs);
    auto* linetypeLayout = new QVBoxLayout(linetypePage);
    impl_->linetypeTable = new QTableWidget(linetypePage);
    impl_->linetypeTable->setColumnCount(4);
    impl_->linetypeTable->setHorizontalHeaderLabels({"Name", "Pattern", "Period", "Description"});
    impl_->linetypeTable->setEditTriggers(QAbstractItemView::NoEditTriggers);
    impl_->linetypeTable->setSelectionBehavior(QAbstractItemView::SelectRows);
    impl_->linetypeTable->setSelectionMode(QAbstractItemView::SingleSelection);
    impl_->linetypeTable->horizontalHeader()->setStretchLastSection(true);
    linetypeLayout->addWidget(impl_->linetypeTable, 1);

    auto* linetypeEditor = new QGroupBox("Pattern", linetypePage);
    linetypeEditor->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Maximum);
    auto* linetypeForm = new QFormLayout(linetypeEditor);
    impl_->patternBox = new QLineEdit(linetypeEditor);
    impl_->patternBox->setPlaceholderText("model units: 1 -0.5 means a 1 m dash then a 0.5 m gap");
    linetypeForm->addRow("Lengths", impl_->patternBox);
    impl_->linetypeDescriptionBox = new QLineEdit(linetypeEditor);
    linetypeForm->addRow("Description", impl_->linetypeDescriptionBox);
    impl_->linetypePreview = new StylePreview(linetypeEditor);
    linetypeForm->addRow("Preview", impl_->linetypePreview);
    linetypeLayout->addWidget(linetypeEditor);

    auto* linetypeButtons = new QWidget(linetypePage);
    auto* linetypeButtonLayout = new QHBoxLayout(linetypeButtons);
    linetypeButtonLayout->setContentsMargins(0, 0, 0, 0);
    auto* newLinetype = new QPushButton("New...", linetypeButtons);
    auto* renameLinetype = new QPushButton("Rename...", linetypeButtons);
    auto* deleteLinetype = new QPushButton("Delete", linetypeButtons);
    auto* applyLinetype = new QPushButton("Save Changes", linetypeButtons);
    auto* assignToLayer = new QPushButton("Assign to Layer...", linetypeButtons);
    renameLinetype->setToolTip(
        "Rename it; every layer and style naming it comes too, as one undo step");
    deleteLinetype->setToolTip("Refused while any layer or style still names it, and says which");
    applyLinetype->setToolTip("Store the lengths above as this linetype's pattern");
    for (QPushButton* button :
         {newLinetype, renameLinetype, deleteLinetype, applyLinetype, assignToLayer}) {
        linetypeButtonLayout->addWidget(button);
    }
    linetypeButtonLayout->addStretch(1);
    linetypeLayout->addWidget(linetypeButtons);
    tabs->addTab(linetypePage, "Linetypes");

    auto* layout = new QVBoxLayout(this);
    layout->addWidget(tabs, 1);
    auto* close = new QDialogButtonBox(QDialogButtonBox::Close, this);
    connect(close, &QDialogButtonBox::rejected, this, &QDialog::reject);
    connect(close, &QDialogButtonBox::accepted, this, &QDialog::accept);
    layout->addWidget(close);

    // ---- wiring ------------------------------------------------------------
    const auto reloadAll = [this] {
        reloadTables();
        loadSelectedStyle();
        loadSelectedLinetype();
    };

    connect(impl_->styleTable, &QTableWidget::itemSelectionChanged, this,
            [this] { loadSelectedStyle(); });
    connect(impl_->linetypeTable, &QTableWidget::itemSelectionChanged, this,
            [this] { loadSelectedLinetype(); });
    connect(impl_->byLayerBox, &QCheckBox::toggled, this, [this](bool byLayer) {
        impl_->colourButton->setEnabled(!byLayer);
        refreshStylePreview();
    });
    connect(impl_->colourButton, &QPushButton::clicked, this, [this] {
        const QColor start = impl_->pickedColour ? toQColor(*impl_->pickedColour) : QColor(Qt::white);
        const QColor picked = QColorDialog::getColor(start, this, "Style Colour");
        if (picked.isValid()) {
            impl_->pickedColour = fromQColor(picked);
            impl_->byLayerBox->setChecked(false);
            refreshStylePreview();
        }
    });
    for (QComboBox* box : {impl_->linetypeBox, impl_->symbolBox, impl_->hatchBox}) {
        connect(box, &QComboBox::currentTextChanged, this, [this] { refreshStylePreview(); });
    }
    connect(impl_->weightBox, &QDoubleSpinBox::valueChanged, this,
            [this] { refreshStylePreview(); });
    connect(impl_->symbolSizeBox, &QDoubleSpinBox::valueChanged, this,
            [this] { refreshStylePreview(); });
    connect(impl_->patternBox, &QLineEdit::textChanged, this,
            [this] { refreshLinetypePreview(); });

    connect(newStyle, &QPushButton::clicked, this, [this, reloadAll] {
        bool accepted = false;
        const QString name = QInputDialog::getText(this, "New Style", "Style name:",
                                                   QLineEdit::Normal, {}, &accepted);
        if (!accepted || name.trimmed().isEmpty()) {
            return;
        }
        Style style;
        style.name = name.trimmed().toStdString();
        if (impl_->run(cmd::createStyle(std::move(style)), "Style " + name.trimmed() + " created.")) {
            reloadAll();
            selectRow(impl_->styleTable, name.trimmed());
        }
    });
    connect(renameStyle, &QPushButton::clicked, this, [this, reloadAll] {
        const std::string from = impl_->selectedStyle();
        if (from.empty()) {
            return;
        }
        bool accepted = false;
        const QString to =
            QInputDialog::getText(this, "Rename Style", "New name:", QLineEdit::Normal,
                                  QString::fromStdString(from), &accepted);
        if (!accepted || to.trimmed().isEmpty()) {
            return;
        }
        if (impl_->run(cmd::renameStyle(from, to.trimmed().toStdString()),
                       QString("Style %1 renamed to %2; every entity wearing it came too.")
                           .arg(QString::fromStdString(from), to.trimmed()))) {
            reloadAll();
            selectRow(impl_->styleTable, to.trimmed());
        }
    });
    connect(deleteStyle, &QPushButton::clicked, this, [this, reloadAll] {
        const std::string name = impl_->selectedStyle();
        if (name.empty()) {
            return;
        }
        if (impl_->run(cmd::deleteStyle(name),
                       "Style " + QString::fromStdString(name) + " deleted.")) {
            reloadAll();
        }
    });
    connect(applyStyle, &QPushButton::clicked, this, [this, reloadAll] {
        const std::string name = impl_->selectedStyle();
        if (name.empty()) {
            return;
        }
        if (impl_->run(cmd::updateStyle(styleFromForm(name)),
                       "Style " + QString::fromStdString(name) + " updated.")) {
            reloadAll();
            selectRow(impl_->styleTable, QString::fromStdString(name));
        }
    });
    connect(applyToSelection, &QPushButton::clicked, this, [this] {
        const std::string name = impl_->selectedStyle();
        if (name.empty()) {
            return;
        }
        const auto ids = impl_->document.selection().ids();
        if (ids.empty()) {
            impl_->log("Select the entities to style first.", true);
            return;
        }
        impl_->run(cmd::setEntityStyle(ids, name),
                   QString("%1 entities set to style %2.")
                       .arg(ids.size())
                       .arg(QString::fromStdString(name)));
    });

    connect(newLinetype, &QPushButton::clicked, this, [this, reloadAll] {
        bool accepted = false;
        const QString name = QInputDialog::getText(this, "New Linetype", "Linetype name:",
                                                   QLineEdit::Normal, {}, &accepted);
        if (!accepted || name.trimmed().isEmpty()) {
            return;
        }
        Linetype linetype;
        linetype.name = name.trimmed().toStdString();
        linetype.description = "User defined";
        if (impl_->run(cmd::createLinetype(std::move(linetype)),
                       "Linetype " + name.trimmed() + " created (solid until given a pattern).")) {
            reloadAll();
            selectRow(impl_->linetypeTable, name.trimmed());
        }
    });
    connect(renameLinetype, &QPushButton::clicked, this, [this, reloadAll] {
        const std::string from = impl_->selectedLinetype();
        if (from.empty()) {
            return;
        }
        bool accepted = false;
        const QString to =
            QInputDialog::getText(this, "Rename Linetype", "New name:", QLineEdit::Normal,
                                  QString::fromStdString(from), &accepted);
        if (!accepted || to.trimmed().isEmpty()) {
            return;
        }
        if (impl_->run(cmd::renameLinetype(from, to.trimmed().toStdString()),
                       QString("Linetype %1 renamed to %2; every layer and style naming it "
                               "came too.")
                           .arg(QString::fromStdString(from), to.trimmed()))) {
            reloadAll();
            selectRow(impl_->linetypeTable, to.trimmed());
        }
    });
    connect(deleteLinetype, &QPushButton::clicked, this, [this, reloadAll] {
        const std::string name = impl_->selectedLinetype();
        if (name.empty()) {
            return;
        }
        if (impl_->run(cmd::deleteLinetype(name),
                       "Linetype " + QString::fromStdString(name) + " deleted.")) {
            reloadAll();
        }
    });
    connect(applyLinetype, &QPushButton::clicked, this, [this, reloadAll] {
        const std::string name = impl_->selectedLinetype();
        if (name.empty()) {
            return;
        }
        const auto linetype = linetypeFromForm(name);
        if (!linetype) {
            impl_->log(QString::fromStdString(linetype.error().describe()), true);
            return;
        }
        if (impl_->run(cmd::updateLinetype(*linetype),
                       "Linetype " + QString::fromStdString(name) + " updated.")) {
            reloadAll();
            selectRow(impl_->linetypeTable, QString::fromStdString(name));
        }
    });
    connect(assignToLayer, &QPushButton::clicked, this, [this] {
        const std::string name = impl_->selectedLinetype();
        if (name.empty()) {
            return;
        }
        QStringList layers;
        for (const std::string& layer : impl_->document.model().layers.names()) {
            layers << QString::fromStdString(layer);
        }
        if (layers.isEmpty()) {
            return;
        }
        bool accepted = false;
        const QString chosen = QInputDialog::getItem(this, "Assign Linetype to Layer",
                                                     "Layer:", layers, 0, false, &accepted);
        if (!accepted || chosen.isEmpty()) {
            return;
        }
        const katana::entity::Layer* current =
            impl_->document.model().layers.find(chosen.toStdString());
        if (current == nullptr) {
            return;
        }
        katana::entity::Layer updated = *current;
        updated.linetype = name;
        impl_->run(cmd::updateLayer(std::move(updated)),
                   QString("Layer %1 now draws with linetype %2.")
                       .arg(chosen, QString::fromStdString(name)));
    });

    reloadAll();
}

StyleManagerDialog::~StyleManagerDialog() = default;

void StyleManagerDialog::reloadTables()
{
    const katana::entity::Model& model = impl_->document.model();

    impl_->styleTable->setRowCount(0);
    for (const Style& style : model.styles.all()) {
        const int row = impl_->styleTable->rowCount();
        impl_->styleTable->insertRow(row);
        const QString colour =
            style.color ? QString::fromStdString(style.color->toHex()) : QString(kByLayer);
        const QString symbol = style.symbol.empty()
                                   ? QString(kNoSymbolLabel)
                                   : QString("%1 @ %2")
                                         .arg(QString::fromStdString(style.symbol))
                                         .arg(style.symbolSize, 0, 'g', 4);
        const QStringList cells{
            QString::fromStdString(style.name), QString::fromStdString(style.linetype),
            QString::number(style.lineWeight, 'f', 2), colour,
            style.hatchPattern.empty() ? QString(kByLayer)
                                       : QString::fromStdString(style.hatchPattern),
            symbol, QString::fromStdString(style.description)};
        for (int column = 0; column < cells.size(); ++column) {
            auto* item = new QTableWidgetItem(cells.at(column));
            if (column == 3 && style.color) {
                item->setForeground(toQColor(*style.color));
            }
            impl_->styleTable->setItem(row, column, item);
        }
    }

    impl_->linetypeTable->setRowCount(0);
    for (const Linetype& linetype : model.linetypes.all()) {
        const int row = impl_->linetypeTable->rowCount();
        impl_->linetypeTable->insertRow(row);
        const QStringList cells{QString::fromStdString(linetype.name), patternText(linetype),
                                QString::number(linetype.patternLength(), 'g', 6),
                                QString::fromStdString(linetype.description)};
        for (int column = 0; column < cells.size(); ++column) {
            impl_->linetypeTable->setItem(row, column, new QTableWidgetItem(cells.at(column)));
        }
    }

    // The combo boxes name what the tables hold, so a style cannot be given a
    // linetype or a hatch that does not exist.
    const QString linetypeWas = impl_->linetypeBox->currentText();
    impl_->linetypeBox->clear();
    for (const std::string& name : model.linetypes.names()) {
        impl_->linetypeBox->addItem(QString::fromStdString(name));
    }
    impl_->linetypeBox->setCurrentText(linetypeWas);

    const QString hatchWas = impl_->hatchBox->currentText();
    impl_->hatchBox->clear();
    impl_->hatchBox->addItem(kByLayer);
    for (const std::string& name : model.hatchPatterns.names()) {
        impl_->hatchBox->addItem(QString::fromStdString(name));
    }
    impl_->hatchBox->setCurrentText(hatchWas);

    if (impl_->symbolBox->count() == 0) {
        impl_->symbolBox->addItem(kNoSymbolLabel);
        for (const std::string_view symbol : katana::entity::symbolNames()) {
            impl_->symbolBox->addItem(
                QString::fromUtf8(symbol.data(), static_cast<int>(symbol.size())));
        }
    }
}

void StyleManagerDialog::loadSelectedStyle()
{
    const std::string name = impl_->selectedStyle();
    const Style* style = impl_->document.model().styles.find(name);
    if (style == nullptr) {
        return;
    }
    impl_->linetypeBox->setCurrentText(QString::fromStdString(style->linetype));
    impl_->weightBox->setValue(style->lineWeight);
    impl_->byLayerBox->setChecked(!style->color.has_value());
    impl_->colourButton->setEnabled(style->color.has_value());
    impl_->pickedColour = style->color;
    impl_->hatchBox->setCurrentText(style->hatchPattern.empty()
                                        ? QString(kByLayer)
                                        : QString::fromStdString(style->hatchPattern));
    impl_->symbolBox->setCurrentText(style->symbol.empty()
                                         ? QString(kNoSymbolLabel)
                                         : QString::fromStdString(style->symbol));
    impl_->symbolSizeBox->setValue(style->symbolSize);
    impl_->descriptionBox->setText(QString::fromStdString(style->description));
    refreshStylePreview();
}

void StyleManagerDialog::loadSelectedLinetype()
{
    const std::string name = impl_->selectedLinetype();
    const Linetype* linetype = impl_->document.model().linetypes.find(name);
    if (linetype == nullptr) {
        return;
    }
    QString lengths;
    for (const auto& element : linetype->pattern) {
        lengths += (lengths.isEmpty() ? "" : " ") + QString::number(element.length, 'g', 6);
    }
    impl_->patternBox->setText(lengths);
    impl_->linetypeDescriptionBox->setText(QString::fromStdString(linetype->description));
    // "continuous" is what an unset ByLayer chain resolves to; it may not be
    // given a pattern, and the form says so rather than failing on Save.
    const bool protectedItem = name == katana::entity::kContinuousLinetype;
    impl_->patternBox->setEnabled(!protectedItem);
    refreshLinetypePreview();
}

void StyleManagerDialog::refreshStylePreview()
{
    const std::string name = impl_->selectedStyle();
    if (name.empty()) {
        return;
    }
    const Style style = styleFromForm(name);
    const Linetype* linetype = impl_->document.model().linetypes.find(style.linetype);
    impl_->stylePreview->setSample(linetype != nullptr ? *linetype : Linetype{}, style,
                                   palette().color(QPalette::WindowText));
}

void StyleManagerDialog::refreshLinetypePreview()
{
    const std::string name = impl_->selectedLinetype();
    if (name.empty()) {
        return;
    }
    // Previewed from the form, not from the table: the point of it is to show
    // what Save Changes would do before it is done.
    const auto linetype = linetypeFromForm(name);
    Style plain;
    plain.name = name;
    impl_->linetypePreview->setSample(linetype ? *linetype : Linetype{}, plain,
                                      palette().color(QPalette::WindowText));
}

katana::entity::Style StyleManagerDialog::styleFromForm(const std::string& name) const
{
    Style style;
    style.name = name;
    style.linetype = impl_->linetypeBox->currentText().toStdString();
    style.lineWeight = impl_->weightBox->value();
    style.color = impl_->byLayerBox->isChecked() ? std::nullopt : impl_->pickedColour;
    const QString hatch = impl_->hatchBox->currentText();
    style.hatchPattern = hatch == kByLayer ? std::string() : hatch.toStdString();
    const QString symbol = impl_->symbolBox->currentText();
    style.symbol =
        symbol == kNoSymbolLabel ? std::string(katana::entity::kNoSymbol) : symbol.toStdString();
    style.symbolSize = impl_->symbolSizeBox->value();
    style.description = impl_->descriptionBox->text().toStdString();
    return style;
}

katana::core::Result<katana::entity::Linetype>
StyleManagerDialog::linetypeFromForm(const std::string& name) const
{
    Linetype linetype;
    linetype.name = name;
    linetype.description = impl_->linetypeDescriptionBox->text().toStdString();
    const QStringList lengths =
        impl_->patternBox->text().split(QRegularExpression("[\\s,]+"), Qt::SkipEmptyParts);
    for (const QString& length : lengths) {
        bool ok = false;
        const double value = length.toDouble(&ok);
        if (!ok) {
            return katana::core::makeError(katana::core::ErrorCode::ParseFailure,
                                           "a pattern length is not a number",
                                           length.toStdString());
        }
        linetype.pattern.push_back(katana::entity::LinetypeElement{value});
    }
    return linetype;
}

void StyleManagerDialog::selectRow(QTableWidget* table, const QString& name)
{
    for (int row = 0; row < table->rowCount(); ++row) {
        if (table->item(row, 0) != nullptr && table->item(row, 0)->text() == name) {
            table->selectRow(row);
            return;
        }
    }
}

void StyleManagerDialog::showFirstRows()
{
    if (impl_->styleTable->rowCount() > 0) {
        impl_->styleTable->selectRow(0);
    }
    if (impl_->linetypeTable->rowCount() > 0) {
        impl_->linetypeTable->selectRow(0);
    }
}

} // namespace katana::qt
