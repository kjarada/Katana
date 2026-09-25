#include "project_crs_dialog.hpp"

#include <map>

#include <QDialogButtonBox>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "katana/cad/project_crs.hpp"

namespace katana::qt {

namespace {

QString qs(const std::string& text) { return QString::fromStdString(text); }

constexpr int kIdRole = Qt::UserRole;

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
                                   std::optional<std::pair<double, double>> place, QWidget* parent)
    : QDialog(parent), document_(document), place_(place)
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
    text_->setText(qs(document_.metadata().coordinateSystem));
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
}

void ProjectCrsDialog::fill(const QString& filter)
{
    list_->clear();
    const auto add = [this](QTreeWidgetItem* parent, const katana::cad::CrsChoice& entry) {
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

QString ProjectCrsDialog::text() const { return text_->text(); }

QString ProjectCrsDialog::check() const { return check_->text(); }

bool ProjectCrsDialog::apply()
{
    const auto status = document_.setCoordinateSystem(text_->text().toStdString());
    if (!status) {
        check_->setText(QStringLiteral("Not set: ") + qs(status.error().describe()));
        return false;
    }
    current_->setText(QStringLiteral("Now: ") + projectCrsLabel(document_));
    return true;
}

bool chooseProjectCrs(QWidget* parent, katana::cad::Document& document,
                      std::optional<std::pair<double, double>> place)
{
    const std::string before = document.metadata().coordinateSystem;
    ProjectCrsDialog dialog(document, place, parent);
    dialog.exec();
    return document.metadata().coordinateSystem != before;
}

} // namespace katana::qt
