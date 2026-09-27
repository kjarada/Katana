// GIS > Check - GDAL > Gaps and Overlaps... (coverage_dialog.hpp).

#include "geo/coverage_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QLineEdit>

#include <utility>

#include "geo/geometry_check_dialog.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

Result<QString> gisCoverageLine(const GisCoverageForm& form)
{
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = QString("GIS COVERAGE ") + (form.clean ? "CLEAN " : "CHECK ") + *scope;
    const auto gap = gisNumberOption("gap", form.gap, "The gap width");
    if (!gap) {
        return gap.error();
    }
    line += *gap;
    if (form.clean) {
        const auto snap = gisNumberOption("snap", form.snap, "The snapping distance");
        if (!snap) {
            return snap.error();
        }
        line += *snap;
        if (!form.merge.trimmed().isEmpty()) {
            line += " merge=" + form.merge.trimmed();
        }
        if (!form.replace) {
            return makeError(ErrorCode::InvalidArgument,
                             "a clean moves boundaries: tick Replace to change the areas in "
                             "place, or Check first");
        }
        return line + " REPLACE";
    }
    if (const QString markers = form.markers.trimmed(); !markers.isEmpty()) {
        const auto word = gisWord("markers=" + markers, "The markers' layer");
        if (!word) {
            return word.error();
        }
        line += " " + *word;
    }
    return line;
}

GisCoverageDialog::GisCoverageDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisCoverage", "Gaps and Overlaps", std::move(context), parent)
{
    mode_ = new QComboBox(this);
    mode_->setObjectName("gisCoverageMode");
    mode_->addItems({"check", "clean"});
    mode_->setToolTip("check reports overlaps, gaps and edges that do not match; clean moves the "
                      "boundaries so the areas meet - enclosed gaps only");
    fields().addRow("Mode:", mode_);
    gap_ = addField("gisCoverageGap", "Gap width (m):", "none - no gaps looked for",
                    "gap=: find (check) or close (clean) the enclosed gaps narrower than this");
    snap_ = addField("gisCoverageSnap", "Snap within (m):", "none",
                     "snap=: clean only - vertices this close together become one");
    merge_ = new QComboBox(this);
    merge_->setObjectName("gisCoverageMerge");
    merge_->addItems({"", "longest-border", "max-area", "min-area", "min-index"});
    merge_->setToolTip("merge=: clean only - which neighbour an overlap goes to; blank is GDAL's "
                       "default (the longest shared border)");
    fields().addRow("Overlaps go to:", merge_);
    replace_ = new QCheckBox("Change the areas in place", this);
    replace_->setObjectName("gisCoverageReplace");
    replace_->setToolTip("REPLACE: a clean moves boundaries. Surveyed legal boundaries must not "
                         "be adjusted silently - Check first; one UNDO puts them back");
    fields().addRow(QString(), replace_);
    markers_ = addField("gisCoverageMarkers", "Markers on layer:", "none - report only",
                        "markers=<layer>: check only - each bad edge drawn on this layer, "
                        "replacing the last check's");
    addProblemsTable();
    connect(mode_, &QComboBox::currentTextChanged, this, [this] { modeChanged(); });
    connect(merge_, &QComboBox::currentTextChanged, this, [this] { refreshCommand(); });
    connect(replace_, &QCheckBox::toggled, this, [this] { refreshCommand(); });
    modeChanged();
}

void GisCoverageDialog::modeChanged()
{
    const bool clean = mode_->currentText() == "clean";
    snap_->setEnabled(clean);
    merge_->setEnabled(clean);
    replace_->setEnabled(clean);
    markers_->setEnabled(!clean);
    refreshCommand();
}

GisCoverageForm GisCoverageDialog::form() const
{
    GisCoverageForm form;
    form.scope = scopeWords();
    form.clean = mode_ != nullptr && mode_->currentText() == "clean";
    form.gap = gap_ != nullptr ? gap_->text() : QString();
    form.snap = snap_ != nullptr ? snap_->text() : QString();
    form.merge = merge_ != nullptr ? merge_->currentText() : QString();
    form.replace = replace_ != nullptr && replace_->isChecked();
    form.markers = markers_ != nullptr ? markers_->text() : QString();
    return form;
}

Result<QString> GisCoverageDialog::command() const
{
    return gisCoverageLine(form());
}

QString GisCoverageDialog::summary(const QString& reply) const
{
    if (reply.contains(" preview=yes") || reply.contains(" ran=no") ||
        reply.contains("coverage mode=clean")) {
        return GisToolDialog::summary(reply);
    }
    return gisProblemsSummary(reply, "No problem: the areas meet edge to edge.");
}

} // namespace katana::qt
