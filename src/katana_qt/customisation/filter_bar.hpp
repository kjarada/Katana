#pragma once

// The row above a manager's table: a search field and a set of exclusive
// chips - "All / Used / Unused / Missing" in the style manager, "All / Rules /
// Unmapped" in the code manager - so every manager filters the same way.
//
// The bar only reports; it filters nothing itself. A dialog keeps its
// filtering in a proxy model or in cad::filterChoices and re-applies it from
// these callbacks, which fire on the user's typing and clicking AND on the
// setters below (a filter is state, not an edit: re-applying it is always
// safe, and a dialog restoring a remembered filter wants its list to follow).
//
// objectNames, for tests and the headless driver: "filterText" for the search
// field, and "filter<Label>" for each chip with everything but the letters
// and digits of its label removed: "filterAll", "filterUnused", and
// "filterNotused3" for "Not used (3)".

#include <functional>
#include <vector>

#include <QString>
#include <QStringList>
#include <QWidget>

class QButtonGroup;
class QLineEdit;
class QToolButton;

namespace katana::qt {

class FilterBar : public QWidget {
  public:
    // `chips` are the labels, in order; the first is checked. An empty list
    // gives a search field alone.
    explicit FilterBar(const QStringList& chips, QWidget* parent = nullptr);

    // The search text as typed (case is the caller's to fold: names are
    // case-sensitive, a search is not - decision D3).
    [[nodiscard]] QString text() const;
    void setText(const QString& text);
    void setPlaceholderText(const QString& text);

    // The index of the checked chip, in the order the labels were given; -1
    // when there are no chips.
    [[nodiscard]] int chip() const;
    [[nodiscard]] QString chipLabel() const;
    // Out-of-range indices are ignored.
    void setChip(int index);
    // A chip's label, changed after construction - "Missing (3)" once the
    // dialog has counted. The objectName stays the original label's.
    void setChipLabel(int index, const QString& label);

    [[nodiscard]] QLineEdit* searchField() const { return search_; }

    // The object name a chip labelled `label` gets.
    [[nodiscard]] static QString chipObjectName(const QString& label);

    // Every change of the text, typed or set.
    std::function<void(const QString& text)> onTextChanged{};
    // Every change of the checked chip, clicked or set.
    std::function<void(int index)> onChipChanged{};

  private:
    QLineEdit* search_ = nullptr;
    QButtonGroup* group_ = nullptr;
    std::vector<QToolButton*> chips_{};
};

} // namespace katana::qt
