// GIS > Analysis - GDAL > Dissolve... (dissolve_dialog.hpp).

#include "geo/dissolve_dialog.hpp"

#include <QCheckBox>
#include <QFormLayout>
#include <QLineEdit>

#include <utility>

namespace katana::qt {

using katana::core::Result;

Result<QString> gisDissolveLine(const GisDissolveForm& form)
{
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = "GIS DISSOLVE " + *scope;
    if (const QString by = form.by.trimmed(); !by.isEmpty()) {
        const auto word = gisWord("by=" + by, "The properties to merge by");
        if (!word) {
            return word.error();
        }
        line += " " + *word;
    }
    if (form.keepIdentical) {
        line += " keep=identical";
    }
    const auto target = gisLayerTarget(form.layer);
    if (!target) {
        return target.error();
    }
    line += *target;
    if (form.replace) {
        line += " REPLACE";
    }
    return line;
}

GisDissolveDialog::GisDissolveDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisDissolve", "Dissolve", std::move(context), parent)
{
    by_ = addField("gisDissolveBy", "Merge by properties:", "none - merge every area",
                   "by=a,b: areas merge only with those whose properties agree - superlots by "
                   "owner, stages by stage");
    keep_ = new QCheckBox("Keep the properties each group shares", this);
    keep_->setObjectName("gisDissolveKeepIdentical");
    keep_->setToolTip("keep=identical: a property every area of a group holds alike is kept on "
                      "the merged area; the rest are left behind");
    connect(keep_, &QCheckBox::toggled, this, [this] { refreshCommand(); });
    fields().addRow(QString(), keep_);
    replace_ = new QCheckBox("Delete the areas merged", this);
    replace_->setObjectName("gisDissolveReplace");
    replace_->setToolTip("REPLACE: the areas that went in are deleted in the same undo step");
    connect(replace_, &QCheckBox::toggled, this, [this] { refreshCommand(); });
    fields().addRow(QString(), replace_);
    layer_ = addField("gisDissolveLayer", "Result layer:", "default gis/dissolve",
                      "TO LAYER: the layer the merged areas are drawn on");
    refreshCommand();
}

GisDissolveForm GisDissolveDialog::form() const
{
    GisDissolveForm form;
    form.scope = scopeWords();
    form.by = by_ != nullptr ? by_->text() : QString();
    form.keepIdentical = keep_ != nullptr && keep_->isChecked();
    form.replace = replace_ != nullptr && replace_->isChecked();
    form.layer = layer_ != nullptr ? layer_->text() : QString();
    return form;
}

Result<QString> GisDissolveDialog::command() const
{
    return gisDissolveLine(form());
}

} // namespace katana::qt
