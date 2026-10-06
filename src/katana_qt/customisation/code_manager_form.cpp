// The survey code manager's rule form: one page per kind of section, reading
// a rule in and writing a rule out, with lint on every keystroke. The form
// edits the BUFFER only, and only through the buttons: a table's own change
// signal never edits anything (docs/cad.md).

#include <utility>

#include <QCheckBox>
#include <QComboBox>
#include <QCompleter>
#include <QFormLayout>
#include <QGridLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStackedWidget>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "customisation/code_manager.hpp"
#include "customisation/code_manager_support.hpp"
#include "customisation/name_picker.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/cad/code_edit.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/tables.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyPipe;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;
using katana::entity::SurveySymbol;
using katana::entity::SurveyTextStyle;

namespace {

constexpr std::array<SurveySection, 9> kSections{
    SurveySection::Map,          SurveySection::VertexSymbol,    SurveySection::VertexTextStyle,
    SurveySection::Pipe,         SurveySection::VertexPipe,      SurveySection::SegmentPipe,
    SurveySection::StringAttribute, SurveySection::VertexAttribute, SurveySection::Tinable,
};

enum Page { MapPage, SymbolPage, TextPage, PipePage, AttributesPage, TinablePage };

[[nodiscard]] int pageFor(SurveySection section)
{
    switch (section) {
    case SurveySection::Map:
        return MapPage;
    case SurveySection::VertexSymbol:
        return SymbolPage;
    case SurveySection::VertexTextStyle:
        return TextPage;
    case SurveySection::Pipe:
    case SurveySection::VertexPipe:
    case SurveySection::SegmentPipe:
        return PipePage;
    case SurveySection::StringAttribute:
    case SurveySection::VertexAttribute:
        return AttributesPage;
    case SurveySection::Tinable:
        return TinablePage;
    }
    return MapPage;
}

[[nodiscard]] std::optional<SurveyPipe>& pipeOf(SurveyRule& rule)
{
    if (rule.section == SurveySection::VertexPipe) {
        return rule.vertexPipe;
    }
    if (rule.section == SurveySection::SegmentPipe) {
        return rule.segmentPipe;
    }
    return rule.pipe;
}

[[nodiscard]] const std::optional<SurveyPipe>& pipeOf(const SurveyRule& rule)
{
    return pipeOf(const_cast<SurveyRule&>(rule));
}

[[nodiscard]] std::vector<katana::entity::SurveyAttribute>& attributesOf(SurveyRule& rule)
{
    return rule.section == SurveySection::VertexAttribute ? rule.vertexAttributes
                                                           : rule.attributes;
}

[[nodiscard]] QString text(const std::string& value)
{
    return QString::fromStdString(value);
}

[[nodiscard]] std::string text(const QString& value)
{
    return value.toStdString();
}

[[nodiscard]] QString numberText(double value)
{
    return value == 0.0 ? QString() : text(katana::core::formatExactReal(value));
}

// A number field: empty is "not given", which the map keeps as 0.
[[nodiscard]] katana::core::Result<double> number(const QLineEdit* field, const char* what)
{
    const std::string typed(katana::core::trimmed(text(field->text())));
    if (typed.empty()) {
        return 0.0;
    }
    const auto value = katana::core::parseFiniteDouble(typed);
    if (!value) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(what) + " \"" + typed + "\" is not a number");
    }
    return *value;
}

QLineEdit* lineField(QWidget* parent, const char* name)
{
    auto* field = new QLineEdit(parent);
    field->setObjectName(QString::fromLatin1(name));
    return field;
}

QComboBox* choiceField(QWidget* parent, const char* name, const QStringList& choices,
                       bool editable = false)
{
    auto* box = new QComboBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    box->addItems(choices);
    box->setEditable(editable);
    if (editable) {
        box->setInsertPolicy(QComboBox::NoInsert);
        // As for the colour field: a typed value is kept in its own case.
        box->completer()->setCaseSensitivity(Qt::CaseSensitive);
    }
    return box;
}

// A combo holding text as written, listed or not.
void setChoice(QComboBox* box, const std::string& value)
{
    const QString shown = text(value);
    const int row = box->findText(shown, Qt::MatchExactly | Qt::MatchCaseSensitive);
    if (row >= 0) {
        box->setCurrentIndex(row);
    } else if (box->isEditable()) {
        box->setCurrentIndex(-1);
        box->setEditText(shown);
    }
}

} // namespace

QWidget* SurveyCodeManagerDialog::buildRuleForm()
{
    auto* form = new QWidget(this);
    form->setObjectName(QStringLiteral("ruleForm"));
    auto* layout = new QVBoxLayout(form);

    formTitle_ = new QLabel(form);
    formTitle_->setObjectName(QStringLiteral("ruleFormTitle"));
    QFont bold = formTitle_->font();
    bold.setBold(true);
    formTitle_->setFont(bold);
    layout->addWidget(formTitle_);

    auto* common = new QFormLayout;
    ruleKey_ = lineField(form, "ruleKey");
    ruleKey_->setToolTip(tr("An exact code (PABB) or a prefix ending in * (WM*); matching is "
                            "exact, letter case included"));
    ruleSection_ = new QComboBox(form);
    ruleSection_->setObjectName(QStringLiteral("ruleSection"));
    for (const SurveySection section : kSections) {
        ruleSection_->addItem(QString::fromLatin1(katana::entity::toString(section)));
    }
    ruleComment_ = lineField(form, "ruleComment");
    common->addRow(tr("Key"), ruleKey_);
    common->addRow(tr("Section"), ruleSection_);
    common->addRow(tr("Description"), ruleComment_);
    layout->addLayout(common);

    rulePages_ = new QStackedWidget(form);
    rulePages_->setObjectName(QStringLiteral("rulePages"));

    // feature: where the code goes and how its line is drawn.
    auto* mapPage = new QWidget(rulePages_);
    auto* mapForm = new QFormLayout(mapPage);
    ruleModel_ = lineField(mapPage, "ruleModel");
    ruleModel_->setToolTip(tr("The layer the code's entities go on"));
    ruleModelStatus_ = new QLabel(mapPage);
    ruleModelStatus_->setObjectName(QStringLiteral("ruleModelStatus"));
    ruleColour_ = new QComboBox(mapPage);
    ruleColour_->setObjectName(QStringLiteral("ruleColour"));
    makeColourField(ruleColour_);
    ruleBreakline_ = choiceField(mapPage, "ruleBreakline", {tr("(not set)"), tr("Line"), tr("Point")});
    ruleLinestyle_ = new NamePicker(context_, katana::cad::NameRole::Linetype, false, mapPage);
    ruleLinestyle_->setObjectName(QStringLiteral("ruleLinestyle"));
    ruleWeight_ = lineField(mapPage, "ruleWeight");
    ruleGroup_ = lineField(mapPage, "ruleGroup");
    mapForm->addRow(tr("Layer"), ruleModel_);
    mapForm->addRow(QString(), ruleModelStatus_);
    mapForm->addRow(tr("Colour"), ruleColour_);
    mapForm->addRow(tr("Line / point"), ruleBreakline_);
    mapForm->addRow(tr("Linestyle"), ruleLinestyle_);
    mapForm->addRow(tr("Weight"), ruleWeight_);
    mapForm->addRow(tr("Group"), ruleGroup_);
    rulePages_->addWidget(mapPage);

    // symbol: the symbol at each point.
    auto* symbolPage = new QWidget(rulePages_);
    auto* symbolForm = new QFormLayout(symbolPage);
    ruleSymbol_ = new NamePicker(context_, katana::cad::NameRole::Symbol, false, symbolPage);
    ruleSymbol_->setObjectName(QStringLiteral("ruleSymbol"));
    ruleSymbolColour_ = new QComboBox(symbolPage);
    ruleSymbolColour_->setObjectName(QStringLiteral("ruleSymbolColour"));
    makeColourField(ruleSymbolColour_);
    ruleSymbolSize_ = lineField(symbolPage, "ruleSymbolSize");
    ruleSymbolSize_->setPlaceholderText(tr("the definition's own size"));
    ruleSymbolRotation_ = lineField(symbolPage, "ruleSymbolRotation");
    ruleSymbolOffset_ = lineField(symbolPage, "ruleSymbolOffset");
    ruleSymbolRaise_ = lineField(symbolPage, "ruleSymbolRaise");
    ruleHide_ = choiceField(symbolPage, "ruleHide", {tr("(not set)"), tr("no"), tr("yes")});
    symbolForm->addRow(tr("Symbol"), ruleSymbol_);
    symbolForm->addRow(tr("Colour"), ruleSymbolColour_);
    symbolForm->addRow(tr("Size"), ruleSymbolSize_);
    symbolForm->addRow(tr("Rotation"), ruleSymbolRotation_);
    symbolForm->addRow(tr("Offset"), ruleSymbolOffset_);
    symbolForm->addRow(tr("Raise"), ruleSymbolRaise_);
    symbolForm->addRow(tr("Hide the line"), ruleHide_);
    rulePages_->addWidget(symbolPage);

    // text: the fields a person sets; the rest are kept.
    auto* textPage = new QWidget(rulePages_);
    auto* textForm = new QFormLayout(textPage);
    ruleTextStyle_ = lineField(textPage, "ruleTextStyle");
    ruleTextColour_ = new QComboBox(textPage);
    ruleTextColour_->setObjectName(QStringLiteral("ruleTextColour"));
    makeColourField(ruleTextColour_);
    ruleTextType_ = choiceField(textPage, "ruleTextType", {QString(), QStringLiteral("paper"),
                                                           QStringLiteral("world")}, true);
    ruleTextSize_ = lineField(textPage, "ruleTextSize");
    ruleTextJustifyX_ = lineField(textPage, "ruleTextJustifyX");
    ruleTextJustifyY_ = lineField(textPage, "ruleTextJustifyY");
    textForm->addRow(tr("Text style"), ruleTextStyle_);
    textForm->addRow(tr("Colour"), ruleTextColour_);
    textForm->addRow(tr("Size type"), ruleTextType_);
    textForm->addRow(tr("Size"), ruleTextSize_);
    textForm->addRow(tr("Justify across"), ruleTextJustifyX_);
    textForm->addRow(tr("Justify up"), ruleTextJustifyY_);
    rulePages_->addWidget(textPage);

    // The three pipe sections.
    auto* pipePage = new QWidget(rulePages_);
    auto* pipeForm = new QFormLayout(pipePage);
    rulePipeJustify_ = choiceField(pipePage, "rulePipeJustify",
                                   {QString(), QStringLiteral("Obvert"), QStringLiteral("Invert"),
                                    QStringLiteral("Centre")},
                                   true);
    rulePipeShape_ = choiceField(pipePage, "rulePipeShape",
                                 {QString(), QStringLiteral("diameter"), QStringLiteral("culvert")},
                                 true);
    rulePipeSize1_ = lineField(pipePage, "rulePipeSize1");
    rulePipeSize1_->setToolTip(tr("A number, or another attribute's name: $PipeDiameter"));
    rulePipeSize2_ = lineField(pipePage, "rulePipeSize2");
    rulePipeActive_ = new QCheckBox(tr("Active"), pipePage);
    rulePipeActive_->setObjectName(QStringLiteral("rulePipeActive"));
    pipeForm->addRow(tr("Justify"), rulePipeJustify_);
    pipeForm->addRow(tr("Shape"), rulePipeShape_);
    pipeForm->addRow(tr("Size 1"), rulePipeSize1_);
    pipeForm->addRow(tr("Size 2"), rulePipeSize2_);
    pipeForm->addRow(QString(), rulePipeActive_);
    rulePages_->addWidget(pipePage);

    // The two attribute sections.
    auto* attributePage = new QWidget(rulePages_);
    auto* attributeLayout = new QVBoxLayout(attributePage);
    attributeLayout->addWidget(new QLabel(
        tr("One attribute a line: <type> <name> = <value>, the type text or integer"),
        attributePage));
    ruleAttributes_ = new QPlainTextEdit(attributePage);
    ruleAttributes_->setObjectName(QStringLiteral("ruleAttributes"));
    attributeLayout->addWidget(ruleAttributes_);
    rulePages_->addWidget(attributePage);

    // surface.
    auto* tinablePage = new QWidget(rulePages_);
    auto* tinableForm = new QFormLayout(tinablePage);
    ruleTinable_ = choiceField(tinablePage, "ruleTinable", {tr("(not set)"), tr("yes"), tr("no")});
    tinableForm->addRow(tr("Goes into a surface"), ruleTinable_);
    rulePages_->addWidget(tinablePage);

    layout->addWidget(rulePages_);

    ruleIssues_ = new QLabel(form);
    ruleIssues_->setObjectName(QStringLiteral("ruleIssues"));
    ruleIssues_->setWordWrap(true);
    layout->addWidget(ruleIssues_);

    auto* buttons = new QGridLayout;
    auto* newButton = new QPushButton(tr("New"), form);
    newButton->setObjectName(QStringLiteral("ruleNew"));
    auto* addButton = new QPushButton(tr("Add"), form);
    addButton->setObjectName(QStringLiteral("ruleAdd"));
    addButton->setToolTip(tr("Add the form's rule after every other rule"));
    ruleUpdate_ = new QPushButton(tr("Save"), form);
    ruleUpdate_->setObjectName(QStringLiteral("ruleUpdate"));
    ruleUpdate_->setToolTip(tr("Replace the selected rule with the form's"));
    ruleDuplicate_ = new QPushButton(tr("Duplicate"), form);
    ruleDuplicate_->setObjectName(QStringLiteral("ruleDuplicate"));
    ruleDelete_ = new QPushButton(tr("Delete"), form);
    ruleDelete_->setObjectName(QStringLiteral("ruleDelete"));
    ruleUp_ = new QPushButton(tr("Move Up"), form);
    ruleUp_->setObjectName(QStringLiteral("ruleUp"));
    ruleUp_->setToolTip(tr("Earlier: among rules of one key, the earlier wins"));
    ruleDown_ = new QPushButton(tr("Move Down"), form);
    ruleDown_->setObjectName(QStringLiteral("ruleDown"));
    buttons->addWidget(newButton, 0, 0);
    buttons->addWidget(addButton, 0, 1);
    buttons->addWidget(ruleUpdate_, 0, 2);
    buttons->addWidget(ruleDuplicate_, 0, 3);
    buttons->addWidget(ruleDelete_, 1, 0);
    buttons->addWidget(ruleUp_, 1, 1);
    buttons->addWidget(ruleDown_, 1, 2);
    layout->addLayout(buttons);
    layout->addStretch(1);

    connect(newButton, &QPushButton::clicked, this, [this] {
        tree_->setCurrentItem(nullptr);
        loadForm(SurveyRule{}, std::nullopt);
        ruleKey_->setFocus();
    });
    connect(addButton, &QPushButton::clicked, this, [this] {
        formButton([this]() -> katana::core::Status {
            auto rule = formRule();
            if (!rule) {
                return rule.error();
            }
            return addRule(std::move(*rule));
        });
    });
    connect(ruleUpdate_, &QPushButton::clicked, this, [this] {
        formButton([this]() -> katana::core::Status {
            auto rule = formRule();
            if (!rule) {
                return rule.error();
            }
            if (!formIndex_) {
                return makeError(ErrorCode::InvalidState, "no rule is selected to save over");
            }
            return replaceRule(*formIndex_, std::move(*rule));
        });
    });
    connect(ruleDuplicate_, &QPushButton::clicked, this, [this] {
        formButton([this]() -> katana::core::Status {
            if (!formIndex_) {
                return makeError(ErrorCode::InvalidState, "no rule is selected to duplicate");
            }
            return duplicateRule(*formIndex_);
        });
    });
    connect(ruleDelete_, &QPushButton::clicked, this, [this] {
        formButton([this]() -> katana::core::Status {
            if (!formIndex_) {
                return makeError(ErrorCode::InvalidState, "no rule is selected to delete");
            }
            return removeRule(*formIndex_);
        });
    });
    connect(ruleUp_, &QPushButton::clicked, this, [this] {
        formButton([this]() -> katana::core::Status {
            if (!formIndex_) {
                return makeError(ErrorCode::InvalidState, "no rule is selected to move");
            }
            return moveRule(*formIndex_, -1);
        });
    });
    connect(ruleDown_, &QPushButton::clicked, this, [this] {
        formButton([this]() -> katana::core::Status {
            if (!formIndex_) {
                return makeError(ErrorCode::InvalidState, "no rule is selected to move");
            }
            return moveRule(*formIndex_, 1);
        });
    });

    // Live checking: every field re-lints the form's rule. Nothing is
    // written to the buffer until a button says so.
    const auto changed = [this] { updateFormIssues(); };
    for (QLineEdit* field : form->findChildren<QLineEdit*>()) {
        connect(field, &QLineEdit::textChanged, this, changed);
    }
    for (QComboBox* box : form->findChildren<QComboBox*>()) {
        connect(box, &QComboBox::currentIndexChanged, this, changed);
    }
    connect(ruleAttributes_, &QPlainTextEdit::textChanged, this, changed);
    connect(rulePipeActive_, &QCheckBox::toggled, this, changed);
    connect(ruleSection_, &QComboBox::currentIndexChanged, this, [this] { showSectionPage(); });
    ruleLinestyle_->onNameChosen = [this](const std::string&) { updateFormIssues(); };
    ruleSymbol_->onNameChosen = [this](const std::string&) { updateFormIssues(); };
    return form;
}

void SurveyCodeManagerDialog::loadForm(const SurveyRule& rule, std::optional<std::size_t> index)
{
    loadingForm_ = true;
    formIndex_ = index;
    formTitle_->setText(index ? tr("Rule #%1").arg(*index) : tr("New rule (not in the map)"));
    ruleKey_->setText(text(rule.key));
    const auto sectionRow = std::find(kSections.begin(), kSections.end(), rule.section);
    ruleSection_->setCurrentIndex(static_cast<int>(sectionRow - kSections.begin()));
    ruleComment_->setText(text(rule.comment));

    ruleModel_->setText(text(rule.model));
    setColourField(ruleColour_, rule.colour);
    ruleBreakline_->setCurrentIndex(!rule.breakline ? 0
                                    : *rule.breakline == SurveyBreakline::Line ? 1
                                                                                : 2);
    // The pickers read the Document's library for what they list and draw,
    // so once the drawing has closed they cannot be loaded. Then each shows
    // the rule's name as plain text and is disabled - what it shows is what
    // formRule() writes, from formLinestyle_ and formSymbol_, and a name
    // typed there could be neither checked nor kept.
    const bool open = document() != nullptr;
    pickersLoaded_ = open;
    const SurveySymbol symbol = rule.symbol.value_or(SurveySymbol{});
    formLinestyle_ = rule.linestyle;
    formSymbol_ = symbol.style;
    for (const auto& [picker, name] :
         {std::pair{ruleLinestyle_, &rule.linestyle}, std::pair{ruleSymbol_, &symbol.style}}) {
        if (open) {
            picker->setCurrentName(*name);
        } else {
            picker->setEditText(text(*name));
            picker->setToolTip(tr("The drawing is closed, so its library cannot be listed: "
                                  "the rule keeps this name."));
        }
        picker->setEnabled(open);
    }
    ruleWeight_->setText(text(rule.weight));
    ruleGroup_->setText(text(rule.group));

    setColourField(ruleSymbolColour_, symbol.colour);
    ruleSymbolSize_->setText(numberText(symbol.size));
    ruleSymbolRotation_->setText(numberText(symbol.rotation));
    ruleSymbolOffset_->setText(numberText(symbol.offset));
    ruleSymbolRaise_->setText(numberText(symbol.raise));
    ruleHide_->setCurrentIndex(!rule.hide ? 0 : *rule.hide ? 2 : 1);

    const SurveyTextStyle style = rule.textStyle.value_or(SurveyTextStyle{});
    ruleTextStyle_->setText(text(style.textstyle));
    setColourField(ruleTextColour_, style.colour);
    setChoice(ruleTextType_, style.type);
    ruleTextSize_->setText(numberText(style.size));
    ruleTextJustifyX_->setText(text(style.justifyX));
    ruleTextJustifyY_->setText(text(style.justifyY));

    const SurveyPipe pipe = pipeOf(rule).value_or(SurveyPipe{});
    setChoice(rulePipeJustify_, pipe.justify);
    setChoice(rulePipeShape_, pipe.shape);
    rulePipeSize1_->setText(text(pipe.size1));
    rulePipeSize2_->setText(text(pipe.size2));
    rulePipeActive_->setChecked(pipe.active);

    SurveyRule copy = rule;
    ruleAttributes_->setPlainText(text(katana::cad::formatAttributeLines(attributesOf(copy))));
    ruleTinable_->setCurrentIndex(!rule.tinable ? 0 : *rule.tinable ? 1 : 2);

    for (QPushButton* button : {ruleUpdate_, ruleDuplicate_, ruleDelete_, ruleUp_, ruleDown_}) {
        button->setEnabled(index.has_value());
    }
    if (index) {
        ruleUp_->setEnabled(*index > 0);
        ruleDown_->setEnabled(*index + 1 < buffer_.size());
    }
    loadingForm_ = false;
    showSectionPage();
}

void SurveyCodeManagerDialog::showSectionPage()
{
    const int row = std::max(0, ruleSection_->currentIndex());
    rulePages_->setCurrentIndex(pageFor(kSections[static_cast<std::size_t>(row)]));
    updateFormIssues();
}

katana::core::Result<SurveyRule> SurveyCodeManagerDialog::formRule() const
{
    const int row = std::max(0, ruleSection_->currentIndex());
    const SurveySection section = kSections[static_cast<std::size_t>(row)];
    // Start from the rule being edited, so that what the form does not show
    // (a text style's slant, a pipe rule's attributes) is kept - unless the
    // section changed, when the old section's fields would be ones the new
    // section cannot carry (the mapfile writer refuses those).
    SurveyRule rule;
    if (formIndex_) {
        if (auto current = buffer_.at(*formIndex_); current && current->section == section) {
            rule = std::move(*current);
        }
    }
    rule.section = section;
    rule.key = text(ruleKey_->text());
    rule.comment = text(ruleComment_->text());

    switch (pageFor(section)) {
    case MapPage: {
        rule.model = text(ruleModel_->text());
        rule.colour = text(ruleColour_->currentText());
        const int breakline = ruleBreakline_->currentIndex();
        rule.breakline = breakline == 1   ? std::optional(SurveyBreakline::Line)
                         : breakline == 2 ? std::optional(SurveyBreakline::Point)
                                          : std::nullopt;
        rule.linestyle = pickersLoaded_ ? ruleLinestyle_->currentName() : formLinestyle_;
        rule.weight = text(ruleWeight_->text());
        rule.group = text(ruleGroup_->text());
        break;
    }
    case SymbolPage: {
        SurveySymbol symbol = rule.symbol.value_or(SurveySymbol{});
        symbol.style = pickersLoaded_ ? ruleSymbol_->currentName() : formSymbol_;
        symbol.colour = text(ruleSymbolColour_->currentText());
        for (const auto& [field, target, what] :
             {std::tuple{ruleSymbolSize_, &symbol.size, "the size"},
              std::tuple{ruleSymbolRotation_, &symbol.rotation, "the rotation"},
              std::tuple{ruleSymbolOffset_, &symbol.offset, "the offset"},
              std::tuple{ruleSymbolRaise_, &symbol.raise, "the raise"}}) {
            auto value = number(field, what);
            if (!value) {
                return value.error();
            }
            *target = *value;
        }
        rule.symbol = symbol;
        const int hide = ruleHide_->currentIndex();
        rule.hide = hide == 0 ? std::nullopt : std::optional(hide == 2);
        break;
    }
    case TextPage: {
        SurveyTextStyle style = rule.textStyle.value_or(SurveyTextStyle{});
        style.textstyle = text(ruleTextStyle_->text());
        style.colour = text(ruleTextColour_->currentText());
        style.type = text(ruleTextType_->currentText());
        auto size = number(ruleTextSize_, "the text size");
        if (!size) {
            return size.error();
        }
        style.size = *size;
        style.justifyX = text(ruleTextJustifyX_->text());
        style.justifyY = text(ruleTextJustifyY_->text());
        rule.textStyle = style;
        break;
    }
    case PipePage: {
        SurveyPipe pipe = pipeOf(rule).value_or(SurveyPipe{});
        pipe.justify = text(rulePipeJustify_->currentText());
        pipe.shape = text(rulePipeShape_->currentText());
        pipe.size1 = text(rulePipeSize1_->text());
        pipe.size2 = text(rulePipeSize2_->text());
        pipe.active = rulePipeActive_->isChecked();
        pipeOf(rule) = pipe;
        break;
    }
    case AttributesPage: {
        auto attributes = katana::cad::parseAttributeLines(text(ruleAttributes_->toPlainText()));
        if (!attributes) {
            return attributes.error();
        }
        attributesOf(rule) = std::move(*attributes);
        break;
    }
    case TinablePage: {
        const int tinable = ruleTinable_->currentIndex();
        rule.tinable = tinable == 0 ? std::nullopt : std::optional(tinable == 1);
        break;
    }
    default:
        break;
    }
    return rule;
}

void SurveyCodeManagerDialog::updateFormIssues()
{
    if (loadingForm_ || ruleIssues_ == nullptr) {
        return;
    }
    const katana::cad::Document* doc = document();
    const auto rule = formRule();
    if (!rule) {
        ruleIssues_->setText(text(rule.error().describe()));
        ruleIssues_->setStyleSheet(QStringLiteral("color: #d9534f;"));
        return;
    }
    const std::size_t index = formIndex_.value_or(buffer_.size());
    const std::vector<katana::cad::LintIssue> issues =
        doc == nullptr
            ? std::vector<katana::cad::LintIssue>{}
            : katana::cad::lintSurveyRule(
                  *rule, index, doc->styleLibrary(),
                  [](std::string_view name) { return katana::archive12d::standardColour(name); },
                  [](std::string_view name) { return katana::entity::isBuiltInSymbolName(name); });

    // The layer, checked as the entity tables check it (lint's
    // InvalidLayerPath), and whether applying the code will make it.
    QString layerStatus;
    bool layerBad = false;
    for (const katana::cad::LintIssue& issue : issues) {
        if (issue.kind == katana::cad::LintKind::InvalidLayerPath) {
            layerStatus = text(issue.message);
            layerBad = true;
        }
    }
    if (!layerBad) {
        if (rule->model.empty()) {
            layerStatus = tr("No layer: the code's entities stay on theirs.");
        } else if (doc != nullptr && doc->model().layers.contains(rule->model)) {
            layerStatus = tr("A valid layer the drawing has.");
        } else {
            layerStatus = tr("A valid layer, created when codes are applied.");
        }
    }
    ruleModelStatus_->setText(layerStatus);
    ruleModelStatus_->setStyleSheet(layerBad ? QStringLiteral("color: #d9534f;") : QString());

    QStringList lines;
    bool anyError = false;
    if (const auto valid = katana::entity::validate(*rule); !valid) {
        lines << text(valid.error().describe());
        anyError = true;
    }
    for (const katana::cad::LintIssue& issue : issues) {
        anyError = anyError || issue.severity == katana::cad::LintSeverity::Error;
        lines << QStringLiteral("%1: %2").arg(
            QString::fromLatin1(katana::cad::toString(issue.severity)), text(issue.message));
    }
    ruleIssues_->setText(lines.isEmpty() ? tr("No issues with this rule.")
                                         : lines.join(QLatin1Char('\n')));
    ruleIssues_->setStyleSheet(anyError ? QStringLiteral("color: #d9534f;")
                               : lines.isEmpty()
                                   ? QString()
                                   : QStringLiteral("color: %1;").arg(kUndefinedNameColour.name()));
}

void SurveyCodeManagerDialog::formButton(const std::function<katana::core::Status()>& edit)
{
    if (const auto status = edit(); !status) {
        ruleIssues_->setText(text(status.error().describe()));
        ruleIssues_->setStyleSheet(QStringLiteral("color: #d9534f;"));
        log(text(status.error().describe()), true);
    }
}

} // namespace katana::qt
