#pragma once

// Saved delimited-file layouts (PLAN.MD 45 slice 4's "saved templates"),
// shared by the import wizard and the point export.
//
// A template is surveyio's own text form of a DelimitedLayout
// (layoutTemplate / parseLayoutTemplate), stored under a name a person gives
// it, in QSettings under survey/templates - per user, not per drawing, because
// a template describes the files one instrument or one office writes, which
// outlive any one drawing. The text is stored, never a half-parsed layout, so
// a template saved by a newer Katana that this one cannot read is refused by
// parseLayoutTemplate when it is chosen, not read as something close to it.

#include <QString>

#include <utility>
#include <vector>

#include "katana/core/error.hpp"

class QComboBox;

namespace katana::qt {

// Name and template text, sorted by name.
[[nodiscard]] std::vector<std::pair<QString, QString>> savedLayoutTemplates();
// InvalidArgument for an empty name or one holding '/' or '\' (QSettings reads
// those as groups); the template text is checked by parseLayoutTemplate first,
// so nothing unreadable is ever saved.
[[nodiscard]] katana::core::Status saveLayoutTemplate(const QString& name, const QString& text);
// NotFound when there is no template of that name.
[[nodiscard]] katana::core::Status deleteLayoutTemplate(const QString& name);

// Refills `box` with "(saved templates)" and then every saved name, keeping
// the current name when it is still there.
void fillTemplateChoice(QComboBox& box);

// Fills `box` now and again after every successful saveLayoutTemplate and
// deleteLayoutTemplate in this process, for as long as the box exists. The
// import wizard and the Export dialog are both kept and both non-modal, so a
// list filled only when its dialog was built offered none of the templates
// saved since - and still offered, with its old text, one deleted since.
// Another Katana saving into the same settings is not seen here: each dialog
// also refills its list when it is shown again.
void keepTemplateChoiceCurrent(QComboBox& box);

} // namespace katana::qt
