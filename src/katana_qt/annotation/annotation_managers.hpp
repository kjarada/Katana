#pragma once

// The Text Style and Label Style managers (Format > Text Styles...,
// Format > Label Styles and Rules...; docs/annotation.md "In the window").
//
// Each is a list of the table's items and a form for the chosen one. Apply
// makes the form's item the table's through the table command - one undo
// step - and a refusal (a template that cannot print, a style still used) is
// shown in the dialog's problem line, word for word what the command line
// would reply. The dialogs hold no copy of the tables: every refresh reads
// the document, so an undo or a verb typed while one is open shows at once.

#include <QDialog>
#include <QString>

#include "katana/cad/document.hpp"
#include "katana/entity/annotation.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QListWidget;
class QPushButton;
class QSpinBox;
class QTableWidget;
class QTabWidget;

namespace katana::qt {

class TextStyleManagerDialog final : public QDialog {
  public:
    explicit TextStyleManagerDialog(katana::cad::Document& document, QWidget* parent = nullptr);

    // Selects a style by name and shows it in the form.
    void select(const QString& name);
    // The form's style, as Apply would store it.
    [[nodiscard]] katana::entity::TextStyle formStyle() const;
    // What the last refused action said; empty after one that worked.
    [[nodiscard]] QString problem() const;

    // The actions, as the buttons run them.
    bool apply();
    bool addStyle();
    bool deleteStyle();

  private:
    void refresh();
    void showStyle(const katana::entity::TextStyle& style);
    void report(const katana::core::Status& status);
    [[nodiscard]] QString current() const;

    katana::cad::Document& document_;
    katana::cad::Document::ListenerHandle listener_;
    QListWidget* list_ = nullptr;
    QLineEdit* font_ = nullptr;
    QDoubleSpinBox* paper_ = nullptr;
    QDoubleSpinBox* width_ = nullptr;
    QDoubleSpinBox* oblique_ = nullptr;
    QCheckBox* bold_ = nullptr;
    QCheckBox* italic_ = nullptr;
    QLineEdit* colour_ = nullptr;
    QCheckBox* mask_ = nullptr;
    QDoubleSpinBox* margin_ = nullptr;
    QCheckBox* readable_ = nullptr;
    QDoubleSpinBox* spacing_ = nullptr;
    QLabel* sample_ = nullptr;
    QLabel* problem_ = nullptr;
    QString shown_;
};

class LabelStyleManagerDialog final : public QDialog {
  public:
    explicit LabelStyleManagerDialog(katana::cad::Document& document, QWidget* parent = nullptr);

    void select(const QString& name);
    [[nodiscard]] katana::entity::LabelStyle formStyle() const;
    [[nodiscard]] QString problem() const;
    // What the last rule run or clear did, as AUTOLABEL replies.
    [[nodiscard]] QString runReport() const;

    bool apply();
    bool addStyle();
    bool deleteStyle();
    bool addDefaults();
    // The rules tab: a rule from its form fields, and running or clearing
    // every enabled rule.
    bool addRule();
    bool deleteRule();
    bool runRules();
    bool clearRules();

  private:
    void refresh();
    void showStyle(const katana::entity::LabelStyle& style);
    void checkTemplate();
    void report(const katana::core::Status& status);
    [[nodiscard]] QString current() const;

    katana::cad::Document& document_;
    katana::cad::Document::ListenerHandle listener_;
    QTabWidget* tabs_ = nullptr;
    QListWidget* list_ = nullptr;
    QComboBox* kind_ = nullptr;
    QLineEdit* template_ = nullptr;
    QComboBox* textStyle_ = nullptr;
    QDoubleSpinBox* paper_ = nullptr;
    QComboBox* placement_ = nullptr;
    QComboBox* orientation_ = nullptr;
    QDoubleSpinBox* offset_ = nullptr;
    QCheckBox* leader_ = nullptr;
    QCheckBox* displace_ = nullptr;
    QSpinBox* priority_ = nullptr;
    QComboBox* marker_ = nullptr;
    QDoubleSpinBox* markerSize_ = nullptr;
    QDoubleSpinBox* minimumLength_ = nullptr;
    QDoubleSpinBox* interval_ = nullptr;
    QDoubleSpinBox* tick_ = nullptr;
    QDoubleSpinBox* tickLength_ = nullptr;
    QLabel* templateCheck_ = nullptr;
    QLabel* problem_ = nullptr;
    QString shown_;

    QTableWidget* rules_ = nullptr;
    QLineEdit* ruleName_ = nullptr;
    QComboBox* ruleStyle_ = nullptr;
    QLineEdit* ruleLayer_ = nullptr;
    QLineEdit* ruleCode_ = nullptr;
    QLineEdit* ruleLabelLayer_ = nullptr;
    QLabel* runReport_ = nullptr;
};

} // namespace katana::qt
