#include "plotting/legend_properties.hpp"

#include <QComboBox>
#include <QFormLayout>
#include <QLabel>

#include "plotting/legend_painter.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using plotting::LegendScope;

namespace {

QString counted(std::size_t count, const char* one, const char* many)
{
    return QString("%1 %2").arg(count).arg(QString::fromLatin1(count == 1 ? one : many));
}

} // namespace

QString legendSummary(const plotting::Legend& legend, LegendScope asked)
{
    const QString entries = counted(legend.entries.size(), "entry", "entries");
    const QString plans = counted(legend.windows, "plan", "plans");
    switch (legend.scope) {
    case LegendScope::ThisSheet:
        return QString("%1 from this sheet's %2").arg(entries, plans);
    case LegendScope::WholeSet:
        return QString("%1 from %2 of the set%3")
            .arg(entries, plans,
                 asked == LegendScope::ThisSheet ? QStringLiteral(" - this sheet has no plan")
                                                 : QString());
    case LegendScope::WholeDrawing:
        return QString("%1 from the whole drawing%2")
            .arg(entries, asked != LegendScope::WholeDrawing
                              ? QStringLiteral(" - no sheet has a plan")
                              : QString());
    }
    return entries;
}

void addLegendProperties(QFormLayout& form, katana::cad::Document& document,
                         const plotting::SheetSet& set, std::size_t sheetIndex,
                         const plotting::Viewport& legend, const SheetSource& source,
                         const std::function<void(const QString&, bool)>& report)
{
    QWidget* parent = form.parentWidget();
    auto* scope = new QComboBox(parent);
    scope->setObjectName(QStringLiteral("sheetLegendScope"));
    scope->addItem(QStringLiteral("What this sheet's plans show"),
                   static_cast<int>(LegendScope::ThisSheet));
    scope->addItem(QStringLiteral("What every sheet's plans show"),
                   static_cast<int>(LegendScope::WholeSet));
    scope->addItem(QStringLiteral("Everything in the drawing"),
                   static_cast<int>(LegendScope::WholeDrawing));
    scope->setCurrentIndex(scope->findData(static_cast<int>(legend.legendScope)));
    scope->setToolTip(QStringLiteral(
        "A sheet with no plan lists what the set's plans show, and a set with none the "
        "whole drawing"));
    // Queued: the edit rebuilds the properties, and so deletes this box; a
    // box deleted inside its own signal goes on to touch itself. A box
    // deleted before the queued call runs takes the call with it.
    const std::string id = legend.id;
    QObject::connect(
        scope, &QComboBox::currentIndexChanged, scope,
        [&document, id, scope, report](int) {
            const auto chosen = static_cast<LegendScope>(scope->currentData().toInt());
            if (auto status = plotting::setLegendScope(document, id, chosen); !status) {
                report(QString::fromStdString(status.error().describe()), true);
            }
        },
        Qt::QueuedConnection);
    form.addRow(QStringLiteral("Lists"), scope);

    auto* summary = new QLabel(parent);
    summary->setObjectName(QStringLiteral("sheetLegendSummary"));
    summary->setWordWrap(true);
    if (const auto gathered = gatherLegend(set, sheetIndex, legend, source)) {
        summary->setText(legendSummary(*gathered, legend.legendScope));
    } else {
        summary->setText(QString::fromStdString(gathered.error().describe()));
    }
    form.addRow(QString(), summary);
}

} // namespace katana::qt
