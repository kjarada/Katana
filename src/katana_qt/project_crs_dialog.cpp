#include "project_crs_dialog.hpp"

#include <map>

#include <QDialogButtonBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QRegularExpression>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/project_crs.hpp"
#include "katana/gis/reproject.hpp"

namespace katana::qt {

namespace {

QString qs(const std::string& text) { return QString::fromStdString(text); }

constexpr int kIdRole = Qt::UserRole;

// "151.21, -33.87" (or with blanks between) as a longitude and a latitude;
// nullopt for anything else. The range is suggestCoordinateSystems's to
// judge, so it can say which number is wrong.
std::optional<std::pair<double, double>> parsePlace(const QString& text)
{
    static const QRegularExpression separators(QStringLiteral("[,\\s]+"));
    const QStringList parts = text.trimmed().split(separators, Qt::SkipEmptyParts);
    if (parts.size() != 2) {
        return std::nullopt;
    }
    bool longitude = false;
    bool latitude = false;
    const std::pair<double, double> place{parts[0].toDouble(&longitude),
                                          parts[1].toDouble(&latitude)};
    return longitude && latitude ? std::optional(place) : std::nullopt;
}

// Where the drawing is, as a longitude and a latitude: its centre, taken
// from the project's system to WGS 84 in the longitude-first order the
// reprojection keeps (gis/reproject.hpp). nullopt for a project with no
// system or an empty drawing, which have no place to give.
std::optional<std::pair<double, double>> drawingPlace(const katana::cad::Document& document)
{
    const std::string& crs = document.metadata().coordinateSystem;
    const katana::geometry::Box2 extent = document.model().entities.bounds();
    if (crs.empty() || extent.empty()) {
        return std::nullopt;
    }
    const katana::geometry::Point2 centre = extent.center();
    const auto lonLat = katana::gis::transformPoint(centre.x, centre.y, crs, "EPSG:4326");
    if (!lonLat) {
        return std::nullopt;
    }
    return std::pair{(*lonLat)[0], (*lonLat)[1]};
}

} // namespace

QString projectCrsLabel(const katana::cad::Document& document)
{
    const std::string& stored = document.metadata().coordinateSystem;
    if (stored.empty()) {
        return QStringLiteral("no coordinate system");
    }
    auto description = katana::cad::describeCoordinateSystem(stored);
    if (!description) {
        return qs(stored) + QStringLiteral(" (not recognised)");
    }
    return qs(description->id) + QStringLiteral("  ") + qs(description->name);
}

ProjectCrsDialog::ProjectCrsDialog(katana::cad::Document& document,
                                   std::optional<std::pair<double, double>> place, QWidget* parent,
                                   CommandRunner runner)
    : QDialog(parent), document_(document), runner_(std::move(runner)), place_(place)
{
    setObjectName(QStringLiteral("projectCrsDialog"));
    setWindowTitle(QStringLiteral("Project Coordinate System"));
    resize(560, 560);
    auto* layout = new QVBoxLayout(this);

    current_ = new QLabel(QStringLiteral("Now: ") + projectCrsLabel(document_), this);
    current_->setObjectName(QStringLiteral("projectCrsCurrent"));
    layout->addWidget(current_);

    search_ = new QLineEdit(this);
    search_->setObjectName(QStringLiteral("projectCrsSearch"));
    search_->setPlaceholderText(QStringLiteral("Search: mga 56, gda2020, utm 55s, 27700 ..."));
    search_->setClearButtonEnabled(true);
    layout->addWidget(search_);

    // A place to suggest systems for, as CRS SUGGEST lon,lat does: the one
    // the dialog was opened with, or else the drawing's centre when the
    // project's system can say where that is.
    placeText_ = new QLineEdit(this);
    placeText_->setObjectName(QStringLiteral("projectCrsPlace"));
    placeText_->setPlaceholderText(QStringLiteral("longitude, latitude - e.g. 151.21, -33.87"));
    showPlace();
    auto* suggestButton = new QPushButton(QStringLiteral("Suggest"), this);
    suggestButton->setObjectName(QStringLiteral("projectCrsSuggest"));
    suggestButton->setToolTip(QStringLiteral("List the systems that suit this place first"));
    auto* placeRow = new QHBoxLayout;
    placeRow->addWidget(new QLabel(QStringLiteral("Suggest for a place:"), this));
    placeRow->addWidget(placeText_, 1);
    placeRow->addWidget(suggestButton);
    layout->addLayout(placeRow);
    connect(suggestButton, &QPushButton::clicked, this, [this] { (void)suggest(); });
    connect(placeText_, &QLineEdit::returnPressed, this, [this] { (void)suggest(); });

    list_ = new QTreeWidget(this);
    list_->setObjectName(QStringLiteral("projectCrsList"));
    list_->setColumnCount(2);
    list_->setHeaderLabels({QStringLiteral("Coordinate system"), QStringLiteral("Code")});
    list_->header()->setStretchLastSection(false);
    list_->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    list_->header()->setSectionResizeMode(1, QHeaderView::ResizeToContents);
    layout->addWidget(list_, 1);

    layout->addWidget(new QLabel(
        QStringLiteral("Or type an EPSG code, WKT or a PROJ string (blank for local coordinates):"),
        this));
    text_ = new QLineEdit(this);
    text_->setObjectName(QStringLiteral("projectCrsText"));
    shownSystem_ = qs(document_.metadata().coordinateSystem);
    text_->setText(shownSystem_);
    layout->addWidget(text_);
    check_ = new QLabel(this);
    check_->setObjectName(QStringLiteral("projectCrsCheck"));
    check_->setWordWrap(true);
    layout->addWidget(check_);

    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, this);
    buttons->button(QDialogButtonBox::Ok)->setObjectName(QStringLiteral("projectCrsApply"));
    buttons->button(QDialogButtonBox::Ok)->setText(QStringLiteral("Set"));
    buttons->button(QDialogButtonBox::Cancel)->setObjectName(QStringLiteral("projectCrsCancel"));
    auto* local = buttons->addButton(QStringLiteral("Local Coordinates"),
                                     QDialogButtonBox::ResetRole);
    local->setObjectName(QStringLiteral("projectCrsClear"));
    local->setToolTip(QStringLiteral("No coordinate system: the drawing's own grid"));
    layout->addWidget(buttons);

    connect(search_, &QLineEdit::textChanged, this, [this](const QString& filter) { fill(filter); });
    connect(list_, &QTreeWidget::currentItemChanged, this, [this](QTreeWidgetItem* item) {
        if (item != nullptr && !item->data(0, kIdRole).toString().isEmpty()) {
            text_->setText(item->data(0, kIdRole).toString());
        }
    });
    connect(list_, &QTreeWidget::itemDoubleClicked, this, [this](QTreeWidgetItem* item) {
        if (item != nullptr && !item->data(0, kIdRole).toString().isEmpty() && apply()) {
            accept();
        }
    });
    connect(text_, &QLineEdit::textChanged, this, [this] { recheck(); });
    connect(local, &QPushButton::clicked, this, [this] {
        text_->clear();
        if (apply()) {
            accept();
        }
    });
    connect(buttons, &QDialogButtonBox::accepted, this, [this] {
        if (apply()) {
            accept();
        }
    });
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);

    fill(QString());
    recheck();
    listener_ = document_.addListener(
        [this](const katana::cad::DocumentChange& change) { follow(change); });
}

void ProjectCrsDialog::showPlace()
{
    const auto known = place_ ? place_ : drawingPlace(document_);
    placeText_->setText(known ? QString("%1, %2")
                                    .arg(known->first, 0, 'f', 6)
                                    .arg(known->second, 0, 'f', 6)
                              : QString());
}

void ProjectCrsDialog::follow(const katana::cad::DocumentChange& change)
{
    // The system is set by an undoable step, which says only History, so
    // every step is looked at - and passed over unless the system moved.
    const bool replaced = change.has(katana::cad::DocumentChange::Replaced);
    const QString stored = qs(document_.metadata().coordinateSystem);
    if (!replaced && stored == shownSystem_) {
        return;
    }
    current_->setText(QStringLiteral("Now: ") + projectCrsLabel(document_));
    // What was typed is kept through a change to this drawing's system; a
    // replaced drawing takes nothing of the last one's, the place its opener
    // gave included.
    if (replaced || text_->text() == shownSystem_) {
        text_->setText(stored); // rechecks
    }
    shownSystem_ = stored;
    if (replaced) {
        place_.reset();
        showPlace();
        fill(search_->text());
        recheck();
    }
}

void ProjectCrsDialog::fill(const QString& filter)
{
    list_->clear();
    const auto add = [](QTreeWidgetItem* parent, const katana::cad::CrsChoice& entry) {
        auto* item = new QTreeWidgetItem(parent, {qs(entry.name), qs(entry.id)});
        item->setData(0, kIdRole, qs(entry.id));
    };
    // What suits the place first, when there is one and no search.
    if (place_ && filter.trimmed().isEmpty()) {
        if (auto suggested = katana::cad::suggestCoordinateSystems(place_->first, place_->second)) {
            auto* top = new QTreeWidgetItem(list_, {QStringLiteral("Suggested for this place")});
            top->setFlags(top->flags() & ~Qt::ItemIsSelectable);
            for (const katana::cad::CrsChoice& entry : *suggested) {
                add(top, entry);
            }
        }
    }
    std::map<std::string, QTreeWidgetItem*> groups;
    std::vector<std::string> order;
    for (const katana::cad::CrsChoice& entry :
         katana::cad::findCoordinateSystems(filter.toStdString())) {
        auto found = groups.find(entry.group);
        if (found == groups.end()) {
            auto* top = new QTreeWidgetItem(list_, {qs(entry.group)});
            top->setFlags(top->flags() & ~Qt::ItemIsSelectable);
            found = groups.emplace(entry.group, top).first;
        }
        add(found->second, entry);
    }
    // A search shows every match; the whole list opens the suggestions and
    // Australia, and leaves the 120 UTM zones folded.
    if (!filter.trimmed().isEmpty()) {
        list_->expandAll();
    } else {
        for (int i = 0; i < list_->topLevelItemCount(); ++i) {
            QTreeWidgetItem* top = list_->topLevelItem(i);
            top->setExpanded(!top->text(0).startsWith(QStringLiteral("World - WGS 84 UTM")));
        }
    }
}

void ProjectCrsDialog::recheck()
{
    const QString typed = text_->text().trimmed();
    if (typed.isEmpty()) {
        check_->setText(QStringLiteral("Local coordinates: no coordinate system. Online data and "
                                       "reprojection need one."));
        return;
    }
    auto description = katana::cad::describeCoordinateSystem(typed.toStdString());
    if (!description) {
        check_->setText(QStringLiteral("Not a coordinate system: ") +
                        qs(description.error().message));
        return;
    }
    QString line = qs(description->id) + QStringLiteral("  ") + qs(description->name) +
                   QStringLiteral(" - ") + qs(description->kind) + QStringLiteral(", ") +
                   qs(description->units);
    if (description->areaOfUse) {
        const auto& area = *description->areaOfUse;
        line += QString(", for %1 to %2 E, %3 to %4 N")
                    .arg(area.west, 0, 'f', 1)
                    .arg(area.east, 0, 'f', 1)
                    .arg(area.south, 0, 'f', 1)
                    .arg(area.north, 0, 'f', 1);
        if (place_ && !area.contains(place_->second, place_->first)) {
            line += QStringLiteral(" - NOT where this place is");
        }
    }
    check_->setText(line);
}

bool ProjectCrsDialog::suggest()
{
    const auto place = parsePlace(placeText_->text());
    if (!place) {
        check_->setText(QStringLiteral("Not a place: type a longitude and a latitude, e.g. "
                                       "151.21, -33.87"));
        return false;
    }
    if (auto valid = katana::cad::suggestCoordinateSystems(place->first, place->second); !valid) {
        check_->setText(QStringLiteral("Not a place: ") + qs(valid.error().message));
        return false;
    }
    place_ = place;
    // The suggestions head the whole list, which a search would narrow.
    if (search_->text().isEmpty()) {
        fill(QString());
    } else {
        search_->clear(); // fills
    }
    recheck();
    return true;
}

QString ProjectCrsDialog::text() const { return text_->text(); }

QString ProjectCrsDialog::check() const { return check_->text(); }

bool ProjectCrsDialog::apply()
{
    // The text goes on the line as typed: CRS SET reads everything after SET
    // verbatim, so a WKT keeps its quotes.
    const QString typed = text_->text().trimmed();
    const QString line =
        typed.isEmpty() ? QStringLiteral("CRS CLEAR") : QStringLiteral("CRS SET ") + typed;
    VerbOutcome outcome;
    if (runner_) {
        outcome = runner_(line);
    } else {
        katana::cad::CommandInterpreter interpreter(document_);
        const auto reply = interpreter.run(line.toStdString());
        outcome.ok = reply.ok();
        if (!reply) {
            outcome.error = qs(reply.error().describe());
        }
    }
    if (!outcome.ok) {
        check_->setText(QStringLiteral("Not set: ") + outcome.error);
        return false;
    }
    // projectCrsCurrent is follow's to update, from the step just taken.
    return true;
}

bool chooseProjectCrs(QWidget* parent, katana::cad::Document& document,
                      std::optional<std::pair<double, double>> place, CommandRunner runner)
{
    const std::string before = document.metadata().coordinateSystem;
    ProjectCrsDialog dialog(document, place, parent, std::move(runner));
    dialog.exec();
    return document.metadata().coordinateSystem != before;
}

} // namespace katana::qt
