// GIS > Analysis - GDAL > Boundary Around Features... (hull_dialog.hpp).

#include "geo/hull_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QLineEdit>

#include <utility>

#include "katana/core/text.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

Result<QString> gisHullLine(const GisHullForm& form)
{
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = "GIS HULL " + *scope;
    if (form.concave) {
        const QString ratio = form.ratio.trimmed();
        const auto value = katana::core::parseFiniteDouble(ratio.toStdString());
        if (!value || *value < 0.0 || *value > 1.0) {
            return makeError(ErrorCode::InvalidArgument,
                             "the ratio is a number from 0 (the tightest) to 1 (the convex hull), "
                             "not '" + ratio.toStdString() + "'");
        }
        line += " concave=" + ratio;
        if (form.holes) {
            line += " holes";
        }
    }
    const auto target = gisLayerTarget(form.layer);
    if (!target) {
        return target.error();
    }
    return line + *target;
}

GisHullDialog::GisHullDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisHull", "Boundary Around Features", std::move(context), parent)
{
    kind_ = new QComboBox(this);
    kind_->setObjectName("gisHullKind");
    kind_->addItems({"convex", "concave"});
    kind_->setToolTip("convex: the tightest boundary with no inward corner; concave: one that "
                      "follows the points in, as far as the ratio lets it");
    fields().addRow("Boundary:", kind_);
    ratio_ = addField("gisHullRatio", "Ratio:", "0.1 - from 0 (tightest) to 1 (convex)",
                      "concave=: GEOS's ratio of the longest edge; 1 gives the convex hull");
    ratio_->setText("0.1");
    holes_ = new QCheckBox("Allow holes", this);
    holes_->setObjectName("gisHullHoles");
    holes_->setToolTip("holes: where the points leave a gap inside, the boundary has a hole");
    fields().addRow(QString(), holes_);
    layer_ = addField("gisHullLayer", "Result layer:", "default gis/hull",
                      "TO LAYER: the layer the boundary is drawn on");
    const auto kindChanged = [this] {
        const bool concave = kind_->currentText() == "concave";
        ratio_->setEnabled(concave);
        holes_->setEnabled(concave);
        refreshCommand();
    };
    connect(kind_, &QComboBox::currentTextChanged, this, kindChanged);
    connect(holes_, &QCheckBox::toggled, this, [this] { refreshCommand(); });
    kindChanged();
}

GisHullForm GisHullDialog::form() const
{
    GisHullForm form;
    form.scope = scopeWords();
    form.concave = kind_ != nullptr && kind_->currentText() == "concave";
    form.ratio = ratio_ != nullptr ? ratio_->text() : QString();
    form.holes = holes_ != nullptr && holes_->isChecked();
    form.layer = layer_ != nullptr ? layer_->text() : QString();
    return form;
}

Result<QString> GisHullDialog::command() const
{
    return gisHullLine(form());
}

} // namespace katana::qt
