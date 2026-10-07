#pragma once

// What a person reads each linework control code by.
//
// The seven controls have two kinds of name. A file, a refusal and CUSTOMISE
// SET call one `arcStart` (entity::lineworkCodeMembers, entity/linework_codes.hpp)
// - a word for a script. A person is shown "Begin curve". THIS is the one list
// of the second kind, keyed by the first, so that a control has one name in
// the window: File > Settings lists the seven spellings under it
// (settings_dialog.cpp), and the controls' one editor, the Survey Code
// Manager's Linework tab, labels its seven fields from it
// (code_manager_tabs.cpp, buildLineworkTab).
// SettingsDialog.TheLineworkControlsAreCalledWhatTheirOneEditorCallsThem reads
// the tab's labels against Settings' page, so a field built without the list
// fails there.
//
// Header only, and nothing of the managers in it: either dialog includes it
// without the other.

#include <array>
#include <string_view>

#include <QString>

namespace katana::qt {

struct LineworkControlLabel {
    std::string_view member; // entity::LineworkCodeMember::name: "arcStart"
    const char* label;       // "Begin curve"
};

// In the order entity::lineworkCodeMembers lists the controls, which is the
// order both dialogs show them in.
inline constexpr std::array<LineworkControlLabel, 7> kLineworkControlLabels{{
    {"start", "Start"},
    {"end", "End"},
    {"close", "Close"},
    {"arcStart", "Begin curve"},
    {"arcEnd", "End curve"},
    {"join", "Join to point"},
    {"rectangle", "Rectangle"},
}};

// The label of the control a file calls `member`. A control this list does
// not know yet - one added to entity::lineworkCodeMembers and not here - is
// shown by the name a file gives it, rather than left out or left blank.
[[nodiscard]] inline QString lineworkControlLabel(std::string_view member)
{
    for (const LineworkControlLabel& each : kLineworkControlLabels) {
        if (each.member == member) {
            return QString::fromLatin1(each.label);
        }
    }
    return QString::fromUtf8(member.data(), static_cast<qsizetype>(member.size()));
}

} // namespace katana::qt
