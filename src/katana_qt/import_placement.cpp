#include "import_placement.hpp"

#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFileInfo>
#include <QGridLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QMessageBox>
#include <QPushButton>
#include <QRadioButton>
#include <QSettings>
#include <QVBoxLayout>

#include "katana/interop/import.hpp"

namespace katana::qt {

namespace {

using katana::cad::ImportPlacement;
using katana::cad::ImportPlacementMode;

QString qs(const std::string& text) { return QString::fromStdString(text); }

// Where remember() keeps the last choice, as its IMPORT word.
constexpr const char* kPlacementKey = "import/placement";

// Survey coordinates run to 10^7 m (a national grid's northings); an offset
// past a thousand times that is a typing slip, not a place.
constexpr double kLargestOffset = 1.0e10;

} // namespace

QString importLine(const QString& path, const ImportPlacement& placement)
{
    const QString word = qs(katana::cad::placementWord(placement));
    return "IMPORT \"" + path + "\"" + (word.isEmpty() ? QString() : " " + word);
}

// ---- the Placement group --------------------------------------------------------------------

ImportPlacementBox::ImportPlacementBox(const katana::geometry::Box2& drawing, const QString& path,
                                       QWidget* parent)
    : QGroupBox(QStringLiteral("Placement"), parent), drawing_(drawing), path_(path)
{
    setObjectName(QStringLiteral("importPlacement"));
    keep_ = new QRadioButton(QStringLiteral("&Keep its own coordinates"), this);
    keep_->setObjectName(QStringLiteral("importPlacementKeep"));
    keep_->setToolTip(QStringLiteral(
        "Where the file says. If that is far from the drawing, you are asked where to put it."));
    local_ = new QRadioButton(QStringLiteral("Move to a &local origin (lower-left corner at 0,0)"),
                              this);
    local_->setObjectName(QStringLiteral("importPlacementLocal"));
    alongside_ = new QRadioButton(
        QStringLiteral("Move &alongside the drawing (lower-left corner to the drawing's)"), this);
    alongside_->setObjectName(QStringLiteral("importPlacementAlongside"));
    offset_ = new QRadioButton(QStringLiteral("Move &by"), this);
    offset_->setObjectName(QStringLiteral("importPlacementOffset"));
    const auto spin = [this](const char* name, const QString& suffix) {
        auto* box = new QDoubleSpinBox(this);
        box->setObjectName(QString::fromLatin1(name));
        box->setRange(-kLargestOffset, kLargestOffset);
        box->setDecimals(3); // the millimetre, as survey coordinates are quoted
        box->setSuffix(suffix);
        return box;
    };
    east_ = spin("importOffsetE", QStringLiteral(" east"));
    north_ = spin("importOffsetN", QStringLiteral(" north"));
    shift_ = new QLabel(this);
    shift_->setObjectName(QStringLiteral("importPlacementShift"));
    shift_->setWordWrap(true);
    shift_->setTextInteractionFlags(Qt::TextSelectableByMouse);

    auto* layout = new QVBoxLayout(this);
    layout->addWidget(keep_);
    layout->addWidget(local_);
    layout->addWidget(alongside_);
    auto* offsetRow = new QHBoxLayout;
    offsetRow->addWidget(offset_);
    offsetRow->addWidget(east_, 1);
    offsetRow->addWidget(north_, 1);
    layout->addLayout(offsetRow);
    layout->addWidget(shift_);

    // The last choice, as its IMPORT word; anything unreadable is Keep.
    ImportPlacement last;
    const QSettings settings;
    if (const auto remembered = katana::cad::parsePlacementWord(
            settings.value(kPlacementKey).toString().toStdString());
        remembered && *remembered) {
        last = **remembered;
    }
    setPlacement(last);
    for (QRadioButton* button : {keep_, local_, alongside_, offset_}) {
        connect(button, &QRadioButton::toggled, this, [this] { refresh(); });
    }
    for (QDoubleSpinBox* box : {east_, north_}) {
        connect(box, &QDoubleSpinBox::valueChanged, this, [this] { refresh(); });
    }
}

ImportPlacement ImportPlacementBox::placement() const
{
    ImportPlacement placement;
    if (local_->isChecked()) {
        placement.mode = ImportPlacementMode::Local;
    } else if (alongside_->isChecked()) {
        placement.mode = ImportPlacementMode::Alongside;
    } else if (offset_->isChecked()) {
        placement.mode = ImportPlacementMode::Offset;
        placement.offset = katana::geometry::Vec2(east_->value(), north_->value());
    }
    return placement;
}

void ImportPlacementBox::setPlacement(const ImportPlacement& placement)
{
    switch (placement.mode) {
    case ImportPlacementMode::Keep:
        keep_->setChecked(true);
        break;
    case ImportPlacementMode::Local:
        local_->setChecked(true);
        break;
    case ImportPlacementMode::Alongside:
        alongside_->setChecked(true);
        break;
    case ImportPlacementMode::Offset:
        offset_->setChecked(true);
        east_->setValue(placement.offset.x);
        north_->setValue(placement.offset.y);
        break;
    }
    refresh();
}

QString ImportPlacementBox::said() const { return shift_->text(); }

void ImportPlacementBox::remember() const
{
    QSettings settings;
    settings.setValue(kPlacementKey, qs(katana::cad::placementWord(placement())));
}

void ImportPlacementBox::refresh()
{
    const ImportPlacement chosen = placement();
    east_->setEnabled(chosen.mode == ImportPlacementMode::Offset);
    north_->setEnabled(chosen.mode == ImportPlacementMode::Offset);
    QString text;
    switch (chosen.mode) {
    case ImportPlacementMode::Keep:
        text = QStringLiteral("The data keeps its own coordinates; if it lands far from the "
                              "drawing, you are asked where to put it.");
        break;
    case ImportPlacementMode::Local:
        text = QStringLiteral("The data is moved as one piece so its lower-left corner sits at "
                              "0,0. The move is read off the file, and logged.");
        break;
    case ImportPlacementMode::Alongside:
        // The drawing is known now; the data's corner only once it is read.
        text = drawing_.empty()
                   ? QStringLiteral("The drawing is empty, so there is nothing to sit beside: "
                                    "the data keeps its own coordinates.")
                   : QString("The data is moved as one piece so its lower-left corner sits on "
                             "the drawing's, at %1,%2.")
                         .arg(drawing_.min.x + 0.0, 0, 'f', 3)
                         .arg(drawing_.min.y + 0.0, 0, 'f', 3);
        break;
    case ImportPlacementMode::Offset:
        text = QString("The data is moved as one piece by %1,%2.")
                   .arg(chosen.offset.x + 0.0, 0, 'f', 3)
                   .arg(chosen.offset.y + 0.0, 0, 'f', 3);
        break;
    }
    if (!path_.isEmpty()) {
        text += "\nTyped: " + importLine(path_, chosen);
    }
    shift_->setText(text);
}

// ---- File > Import's step -------------------------------------------------------------------

ImportPlacementDialog::ImportPlacementDialog(const QString& path,
                                             const katana::geometry::Box2& drawing,
                                             QWidget* parent)
    : QDialog(parent), path_(path)
{
    setObjectName(QStringLiteral("importPlacementDialog"));
    setWindowTitle("Import - " + QFileInfo(path).fileName());
    auto* layout = new QVBoxLayout(this);
    box_ = new ImportPlacementBox(drawing, path, this);
    layout->addWidget(box_);
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, this);
    QPushButton* importButton = buttons->button(QDialogButtonBox::Ok);
    importButton->setText(QStringLiteral("Import"));
    importButton->setObjectName(QStringLiteral("importPlacementImport"));
    buttons->button(QDialogButtonBox::Cancel)->setObjectName(QStringLiteral("importPlacementCancel"));
    connect(buttons, &QDialogButtonBox::accepted, this, [this] {
        box_->remember();
        accept();
    });
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    layout->addWidget(buttons);
}

QString ImportPlacementDialog::line() const { return importLine(path_, box_->placement()); }

// ---- deciding -------------------------------------------------------------------------------

PlacementDecision decideImportPlacement(QWidget* parent, bool headless,
                                        const ImportPlacement& placement,
                                        const katana::geometry::Box2& drawing,
                                        const katana::geometry::Box2& incoming,
                                        const std::function<void(const QString&, bool)>& log)
{
    PlacementDecision decision;
    if (placement.mode != ImportPlacementMode::Keep) {
        const katana::cad::ImportShift resolved =
            katana::cad::resolveImportShift(placement, drawing, incoming);
        decision.shift = resolved.shift;
        log(qs(resolved.said), false);
        return decision;
    }
    // Survey data in a projected CRS carries coordinates like (255440,
    // 7410850) while a drawing started from scratch sits near the origin.
    // Merging them succeeds and leaves one of the two a dot smaller than a
    // pixel, so the choice is put BEFORE anything is added rather than left
    // to be found by zooming to extents.
    const auto advice = katana::interop::advisePlacement(drawing, incoming);
    if (!advice.farApart) {
        return decision;
    }
    if (headless) {
        log(qs(advice.message) + " (kept: no one to ask).", true);
        return decision;
    }
    QMessageBox box(parent);
    box.setIcon(QMessageBox::Question);
    box.setWindowTitle(QStringLiteral("Far from the current drawing"));
    box.setText(qs(advice.message) + ".");
    box.setInformativeText(
        QString("The file covers %1,%2 to %3,%4.\n\n"
                "Shifting moves everything it holds as one piece so its lower-left corner sits "
                "on the drawing's; its shape and internal dimensions are unchanged. IMPORT ... "
                "ALONGSIDE does the same without asking.")
            .arg(incoming.min.x, 0, 'f', 2)
            .arg(incoming.min.y, 0, 'f', 2)
            .arg(incoming.max.x, 0, 'f', 2)
            .arg(incoming.max.y, 0, 'f', 2));
    QPushButton* shift = box.addButton(QStringLiteral("Shift Alongside"), QMessageBox::AcceptRole);
    box.addButton(QStringLiteral("Keep Survey Coordinates"), QMessageBox::DestructiveRole);
    QPushButton* cancel = box.addButton(QMessageBox::Cancel);
    box.setDefaultButton(shift);
    box.exec();
    if (box.clickedButton() == cancel) {
        decision.cancelled = true;
        return decision;
    }
    if (box.clickedButton() == shift) {
        const katana::cad::ImportShift resolved = katana::cad::resolveImportShift(
            ImportPlacement{ImportPlacementMode::Alongside, {}}, drawing, incoming);
        decision.shift = resolved.shift;
        log(qs(resolved.said), false);
        return decision;
    }
    log(qs(advice.message) + ".", true);
    return decision;
}

} // namespace katana::qt
