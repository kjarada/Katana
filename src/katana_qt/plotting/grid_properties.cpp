#include "plotting/grid_properties.hpp"

#include <algorithm>
#include <array>
#include <string_view>
#include <utility>

#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QString>

#include "katana/cad/plotting/plan_grid.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using plotting::GridStyle;

void addGridRows(QFormLayout& form, QWidget* parent, const plotting::Viewport& viewport,
                 double automaticIntervalM, std::function<void(GridStyle, double)> apply)
{
    auto* style = new QComboBox(parent);
    style->setObjectName(QStringLiteral("sheetGridStyle"));
    // Each item carries the style's stored name, so the list's wording can
    // change without touching what is saved.
    constexpr std::array<std::pair<const char*, GridStyle>, 4> kStyles{{
        {"None", GridStyle::None},
        {"Ticks at the edges", GridStyle::Ticks},
        {"Crosses", GridStyle::Crosses},
        {"Lines", GridStyle::Lines},
    }};
    for (const auto& [name, value] : kStyles) {
        const std::string_view id = plotting::toString(value);
        style->addItem(QString::fromLatin1(name),
                       QString::fromUtf8(id.data(), static_cast<qsizetype>(id.size())));
    }
    const std::string_view current = plotting::toString(viewport.gridStyle);
    const QString currentId =
        QString::fromUtf8(current.data(), static_cast<qsizetype>(current.size()));
    style->setCurrentIndex(std::max(0, style->findData(currentId)));
    style->setToolTip(QStringLiteral(
        "The ground's eastings and northings, labelled where they meet the view's edges"));

    auto* interval = new QDoubleSpinBox(parent);
    interval->setObjectName(QStringLiteral("sheetGridInterval"));
    interval->setRange(0.0, 1e6);
    interval->setDecimals(3);
    interval->setSuffix(QStringLiteral(" m"));
    // At its minimum, 0, the box reads "Auto".
    interval->setSpecialValueText(QStringLiteral("Auto"));
    // One step when the typing is done, not one per key.
    interval->setKeyboardTracking(false);
    interval->setValue(viewport.gridInterval);
    interval->setEnabled(viewport.gridStyle != GridStyle::None);
    interval->setToolTip(
        automaticIntervalM > 0.0
            ? QString("The spacing in metres; Auto is about 50 mm apart on the paper - %1 m now")
                  .arg(automaticIntervalM)
            : QStringLiteral("The spacing in metres; Auto is about 50 mm apart on the paper"));

    const auto commit = [style, interval, apply = std::move(apply)] {
        const auto chosen = plotting::gridStyleFrom(style->currentData().toString().toStdString());
        const double spacing = interval->value();
        apply(chosen.value_or(GridStyle::None), spacing);
    };
    QObject::connect(style, &QComboBox::currentIndexChanged, style, [commit](int) { commit(); });
    QObject::connect(interval, &QDoubleSpinBox::valueChanged, interval,
                     [commit](double) { commit(); });
    form.addRow(QStringLiteral("Grid"), style);
    form.addRow(QStringLiteral("Grid interval"), interval);
}

} // namespace katana::qt
