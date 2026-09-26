// GIS > Analysis - GDAL > Buffer... (buffer_dialog.hpp).

#include "geo/buffer_dialog.hpp"

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

namespace {

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

QString textOf(const QLineEdit* edit)
{
    return edit != nullptr ? edit->text() : QString();
}

QString choiceOf(const QComboBox* box, const QString& fallback)
{
    return box != nullptr && !box->currentText().isEmpty() ? box->currentText() : fallback;
}

} // namespace

Result<QString> gisBufferLine(const GisBufferForm& form)
{
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = "GIS BUFFER " + *scope;
    const QString property = form.distanceProperty.trimmed();
    if (!property.isEmpty()) {
        const auto word = gisWord("distance=prop:" + property, "The distance property");
        if (!word) {
            return word.error();
        }
        line += " " + *word;
    } else {
        const QString distance = form.distance.trimmed();
        if (distance.isEmpty()) {
            return invalid("give the distance, or the property that holds each one");
        }
        const auto value = katana::core::parseFiniteDouble(distance.toStdString());
        if (!value || *value == 0.0) {
            return invalid("the distance must be a number of metres other than 0, not '" +
                           distance + "'");
        }
        line += " distance=" + distance;
    }
    if (form.side != "both") {
        line += " side=" + form.side;
    }
    if (form.caps != "round") {
        line += " caps=" + form.caps;
    }
    if (form.joins != "round") {
        line += " joins=" + form.joins;
    }
    if (form.dissolve) {
        const QString by = form.dissolveBy.trimmed();
        if (by.isEmpty()) {
            line += " DISSOLVE";
        } else {
            const auto word = gisWord("dissolve=" + by, "The properties to merge by");
            if (!word) {
                return word.error();
            }
            line += " " + *word;
        }
    }
    const auto target = gisLayerTarget(form.layer);
    if (!target) {
        return target.error();
    }
    return line + *target;
}

GisBufferDialog::GisBufferDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisBuffer", "Buffer", std::move(context), parent)
{
    const auto choice = [this](const char* name, const QStringList& items, const QString& tip) {
        auto* box = new QComboBox(this);
        box->setObjectName(name);
        box->addItems(items);
        box->setToolTip(tip);
        connect(box, &QComboBox::currentTextChanged, this, [this] { refreshCommand(); });
        return box;
    };
    distance_ = addField("gisBufferDistance", "Distance (m):", "e.g. 5; negative shrinks an area",
                         "distance=: how far out from each entity - an easement's half width, "
                         "a clearance; a negative distance sets an area in (a setback)");
    property_ = addField("gisBufferDistanceProperty", "Or distance from property:",
                         "none - the distance above for every entity",
                         "distance=prop:<key>: each entity buffered by the number its own "
                         "property holds (a clearance by voltage); one without it is skipped "
                         "and counted");
    side_ = choice("gisBufferSide", {"both", "left", "right"},
                   "side=: both sides of a line, or only its left or right, walking along it");
    fields().addRow("Side:", side_);
    caps_ = choice("gisBufferCaps", {"round", "flat", "square"},
                   "caps=: how a line's ends are finished; a round buffer of a point is drawn "
                   "as an exact circle");
    fields().addRow("Line ends:", caps_);
    joins_ = choice("gisBufferJoins", {"round", "mitre", "bevel"},
                    "joins=: how corners are finished; mitre keeps a lot's corners square");
    fields().addRow("Corners:", joins_);
    dissolve_ = new QCheckBox("Merge the results", this);
    dissolve_->setObjectName("gisBufferDissolve");
    dissolve_->setToolTip("DISSOLVE: one area where the buffers overlap, instead of one each");
    connect(dissolve_, &QCheckBox::toggled, this, [this] { refreshCommand(); });
    fields().addRow(QString(), dissolve_);
    dissolveBy_ = addField("gisBufferDissolveBy", "Merge by properties:",
                           "none - merge everything",
                           "dissolve=a,b: merge only results whose properties agree");
    layer_ = addField("gisBufferLayer", "Result layer:", "default gis/buffer",
                      "TO LAYER: the layer the areas are drawn on");
    refreshCommand();
}

GisBufferForm GisBufferDialog::form() const
{
    GisBufferForm form;
    form.scope = scopeWords();
    form.distance = textOf(distance_);
    form.distanceProperty = textOf(property_);
    form.side = choiceOf(side_, "both");
    form.caps = choiceOf(caps_, "round");
    form.joins = choiceOf(joins_, "round");
    form.dissolve = dissolve_ != nullptr && dissolve_->isChecked();
    form.dissolveBy = textOf(dissolveBy_);
    form.layer = textOf(layer_);
    return form;
}

Result<QString> GisBufferDialog::command() const
{
    return gisBufferLine(form());
}

} // namespace katana::qt
