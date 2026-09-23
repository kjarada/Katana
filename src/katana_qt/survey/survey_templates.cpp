#include "survey/survey_templates.hpp"

#include <QComboBox>
#include <QPointer>
#include <QSettings>

#include <algorithm>

#include "katana/surveyio/delimited_points.hpp"

namespace katana::qt {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;

constexpr const char* kGroup = "survey/templates";

// Every choice keepTemplateChoiceCurrent was given. QPointer, because the
// dialogs holding them are destroyed with the window, before this list: a
// box that has gone leaves a null behind, never a dangling pointer.
std::vector<QPointer<QComboBox>>& keptChoices()
{
    static std::vector<QPointer<QComboBox>> choices;
    return choices;
}

// After a save or a delete. Each box is refilled with its signals blocked
// (fillTemplateChoice), so no dialog re-applies a template it did not ask
// for, and none is refilled inside a signal of its own.
void refillKeptChoices()
{
    std::vector<QPointer<QComboBox>>& choices = keptChoices();
    std::erase_if(choices, [](const QPointer<QComboBox>& box) { return box.isNull(); });
    for (const QPointer<QComboBox>& box : choices) {
        fillTemplateChoice(*box);
    }
}

} // namespace

std::vector<std::pair<QString, QString>> savedLayoutTemplates()
{
    QSettings settings;
    settings.beginGroup(kGroup);
    std::vector<std::pair<QString, QString>> templates;
    for (const QString& name : settings.childKeys()) {
        templates.emplace_back(name, settings.value(name).toString());
    }
    std::ranges::sort(templates, {}, &std::pair<QString, QString>::first);
    return templates;
}

katana::core::Status saveLayoutTemplate(const QString& name, const QString& text)
{
    const QString trimmed = name.trimmed();
    if (trimmed.isEmpty() || trimmed.contains('/') || trimmed.contains('\\')) {
        return makeError(ErrorCode::InvalidArgument,
                         "a template needs a name, without '/' or '\\'", name.toStdString());
    }
    if (auto layout = katana::surveyio::parseLayoutTemplate(text.toStdString()); !layout) {
        return layout.error();
    }
    QSettings settings;
    settings.beginGroup(kGroup);
    settings.setValue(trimmed, text);
    settings.sync();
    if (settings.status() != QSettings::NoError) {
        return makeError(ErrorCode::FileExportFailure, "the template could not be saved in the settings",
                         trimmed.toStdString());
    }
    refillKeptChoices();
    return {};
}

katana::core::Status deleteLayoutTemplate(const QString& name)
{
    QSettings settings;
    settings.beginGroup(kGroup);
    if (!settings.contains(name)) {
        return makeError(ErrorCode::NotFound, "no saved template of that name",
                         name.toStdString());
    }
    settings.remove(name);
    settings.sync();
    refillKeptChoices();
    return {};
}

void fillTemplateChoice(QComboBox& box)
{
    const QString current = box.currentIndex() > 0 ? box.currentText() : QString();
    box.blockSignals(true);
    box.clear();
    box.addItem("(saved templates)");
    for (const auto& [name, text] : savedLayoutTemplates()) {
        box.addItem(name, text);
        box.setItemData(box.count() - 1, text, Qt::ToolTipRole);
    }
    const int index = current.isEmpty() ? 0 : box.findText(current);
    box.setCurrentIndex(index < 0 ? 0 : index);
    box.blockSignals(false);
}

void keepTemplateChoiceCurrent(QComboBox& box)
{
    fillTemplateChoice(box);
    keptChoices().emplace_back(&box);
}

} // namespace katana::qt
