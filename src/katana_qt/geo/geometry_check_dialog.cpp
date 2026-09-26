// GIS > Check - GDAL > Check Geometry... and Repair Geometry...
// (geometry_check_dialog.hpp).

#include "geo/geometry_check_dialog.hpp"

#include <QComboBox>
#include <QFormLayout>
#include <QLineEdit>

#include <set>
#include <utility>

#include "geo/replies.hpp"

namespace katana::qt {

using katana::core::Result;

Result<QString> gisCheckLine(const GisCheckForm& form)
{
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = "GIS CHECK " + *scope;
    if (const QString markers = form.markers.trimmed(); !markers.isEmpty()) {
        const auto word = gisWord("markers=" + markers, "The markers' layer");
        if (!word) {
            return word.error();
        }
        line += " " + *word;
    }
    return line;
}

Result<QString> gisRepairLine(const GisRepairForm& form)
{
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = "GIS REPAIR " + *scope;
    if (form.method != "linework") {
        line += " method=" + form.method;
    }
    return line;
}

QString gisProblemsSummary(const QString& reply, const QString& none)
{
    std::size_t problems = 0;
    std::set<std::string> entities;
    for (const auto& record : katana::app::geo::parseRecords(reply.toStdString())) {
        if (record.kind == "problem") {
            ++problems;
            if (const auto entity = record.get("entity"); entity && !entity->empty()) {
                entities.insert(*entity);
            }
        }
    }
    if (problems == 0) {
        return none;
    }
    return QString("%1 problems on %2 entities: each is a row of the table, and a problem "
                   "record in the reply.")
        .arg(problems)
        .arg(entities.size());
}

GisCheckDialog::GisCheckDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisCheck", "Check Geometry", std::move(context), parent)
{
    markers_ = addField("gisCheckMarkers", "Markers on layer:", "none - report only",
                        "markers=<layer>: a point at each problem on this layer, replacing the "
                        "points the last check drew there");
    addProblemsTable();
    refreshCommand();
}

GisCheckForm GisCheckDialog::form() const
{
    GisCheckForm form;
    form.scope = scopeWords();
    form.markers = markers_ != nullptr ? markers_->text() : QString();
    return form;
}

Result<QString> GisCheckDialog::command() const
{
    return gisCheckLine(form());
}

QString GisCheckDialog::summary(const QString& reply) const
{
    if (reply.contains(" preview=yes") || reply.contains(" ran=no")) {
        return GisToolDialog::summary(reply);
    }
    return gisProblemsSummary(reply, "No problem: every line and area in the scope is valid.");
}

GisRepairDialog::GisRepairDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisRepair", "Repair Geometry", std::move(context), parent)
{
    method_ = new QComboBox(this);
    method_->setObjectName("gisRepairMethod");
    method_->addItems({"linework", "structure"});
    method_->setToolTip("method=: linework keeps every edge the area had and nodes them; "
                        "structure rebuilds it from its rings, dropping what is inside out");
    connect(method_, &QComboBox::currentTextChanged, this, [this] { refreshCommand(); });
    fields().addRow("Method:", method_);
    refreshCommand();
}

GisRepairForm GisRepairDialog::form() const
{
    GisRepairForm form;
    form.scope = scopeWords();
    form.method = method_ != nullptr ? method_->currentText() : QString("linework");
    return form;
}

Result<QString> GisRepairDialog::command() const
{
    return gisRepairLine(form());
}

} // namespace katana::qt
