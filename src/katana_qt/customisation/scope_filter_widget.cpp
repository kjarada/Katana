#include "customisation/scope_filter_widget.hpp"

#include <algorithm>
#include <set>
#include <string>

#include <QButtonGroup>
#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QListWidget>
#include <QRadioButton>
#include <QSignalBlocker>
#include <QVBoxLayout>

#include "katana/entity/model.hpp"

namespace katana::qt {

namespace {

using katana::cad::ModifyFilter;
using katana::cad::ModifyScope;
using katana::cad::ScopeKind;
using katana::cad::ScopeSource;
using katana::cad::ScopeWords;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::EntityType;

// Every geometry kind, in the variant's order: the type boxes of the filter.
constexpr EntityType kTypes[] = {EntityType::Point,    EntityType::Line,   EntityType::Arc,
                                 EntityType::Polyline, EntityType::Circle, EntityType::Text,
                                 EntityType::Dimension};

std::string text(const QString& value)
{
    return value.trimmed().toStdString();
}

katana::core::Error noAreaOnScreen(const ScopeFilterView& view)
{
    return makeError(ErrorCode::InvalidArgument,
                     "only a plan view has an area on screen to limit to",
                     view.title.toStdString());
}

} // namespace

std::vector<ScopeFilterView> scopeFilterViews(katana::cad::ViewSet& views)
{
    std::vector<ScopeFilterView> open;
    for (katana::cad::ViewState* state : views.views()) {
        ScopeFilterView view;
        view.id = state->id;
        view.title = QString::fromStdString(katana::cad::ViewSet::title(*state));
        view.hidden = &state->layers;
        if (state->kind == katana::cad::ViewKind::Plan) {
            view.onScreen = state->plan.visibleWorldBounds();
        }
        open.push_back(std::move(view));
    }
    return open;
}

ScopeFilterWidget::ScopeFilterWidget(const QString& namePrefix, QWidget* parent)
    : QWidget(parent), prefix_(namePrefix)
{
    setObjectName(prefix_ + QStringLiteral("Scope"));
    const auto named = [this](QObject* object, const char* part) {
        object->setObjectName(prefix_ + QString::fromLatin1(part));
    };
    auto* outer = new QVBoxLayout(this);
    outer->setContentsMargins(0, 0, 0, 0);

    // ---- Apply to ----
    auto* scopeBox = new QGroupBox(QStringLiteral("Apply to"), this);
    named(scopeBox, "ScopeGroup");
    auto* grid = new QGridLayout(scopeBox);
    auto* group = new QButtonGroup(scopeBox);
    const auto radio = [&](const QString& label, const char* part, int row) {
        auto* button = new QRadioButton(label, scopeBox);
        named(button, part);
        group->addButton(button);
        grid->addWidget(button, row, 0);
        QObject::connect(button, &QRadioButton::toggled, this, [this](bool on) {
            if (on) {
                updateEnabled();
                changed();
            }
        });
        return button;
    };
    scopeSelection_ = radio(QStringLiteral("Selected entities"), "ScopeSelection", 0);
    scopeView_ = radio(QStringLiteral("What a view shows"), "ScopeView", 1);
    view_ = new QComboBox(scopeBox);
    named(view_, "View");
    grid->addWidget(view_, 1, 1);
    onScreen_ = new QCheckBox(QStringLiteral("Only what is on screen"), scopeBox);
    named(onScreen_, "OnScreen");
    onScreen_->setToolTip(QStringLiteral("A plan view: only what lies in the area it shows now"));
    grid->addWidget(onScreen_, 2, 1);
    scopeLayers_ = radio(QStringLiteral("The checked layers"), "ScopeLayers", 3);
    sublayers_ = new QCheckBox(QStringLiteral("With their sublayers"), scopeBox);
    named(sublayers_, "Sublayers");
    sublayers_->setChecked(true);
    grid->addWidget(sublayers_, 3, 1);
    layers_ = new QListWidget(scopeBox);
    named(layers_, "Layers");
    layers_->setMinimumHeight(90);
    grid->addWidget(layers_, 4, 0, 1, 2);
    scopeDrawing_ = radio(QStringLiteral("The whole drawing"), "ScopeDrawing", 5);

    QObject::connect(view_, &QComboBox::currentIndexChanged, this, [this] { changed(); });
    QObject::connect(onScreen_, &QCheckBox::toggled, this, [this] { changed(); });
    QObject::connect(sublayers_, &QCheckBox::toggled, this, [this] { changed(); });
    QObject::connect(layers_, &QListWidget::itemChanged, this, [this] { changed(); });
    outer->addWidget(scopeBox);

    // ---- Only those that match ----
    auto* filterBox = new QGroupBox(QStringLiteral("Only those that match (optional)"), this);
    named(filterBox, "FilterGroup");
    auto* form = new QFormLayout(filterBox);

    auto* typeRow = new QWidget(filterBox);
    auto* typeLayout = new QGridLayout(typeRow);
    typeLayout->setContentsMargins(0, 0, 0, 0);
    int column = 0;
    int row = 0;
    for (const EntityType type : kTypes) {
        const std::string_view spelt = katana::entity::toString(type);
        const QString name =
            QString::fromUtf8(spelt.data(), static_cast<qsizetype>(spelt.size()));
        auto* check = new QCheckBox(name, typeRow);
        check->setObjectName(prefix_ + QStringLiteral("Type") + name);
        QObject::connect(check, &QCheckBox::toggled, this, [this] { changed(); });
        typeLayout->addWidget(check, row, column);
        types_.emplace_back(type, check);
        if (++column == 4) {
            column = 0;
            ++row;
        }
    }
    form->addRow(QStringLiteral("Types (none: all)"), typeRow);

    const auto lineEdit = [this](QWidget* parentWidget, const char* part,
                                 const QString& placeholder) {
        auto* edit = new QLineEdit(parentWidget);
        edit->setObjectName(prefix_ + QString::fromLatin1(part));
        edit->setPlaceholderText(placeholder);
        QObject::connect(edit, &QLineEdit::textChanged, this, [this] { changed(); });
        return edit;
    };
    const QString wild = QStringLiteral(" - * and ? are wildcards");
    filterLayer_ = lineEdit(filterBox, "FilterLayer", QStringLiteral("e.g. survey/*, roads"));
    filterLayer_->setToolTip(QStringLiteral("Layer paths, comma separated") + wild);
    form->addRow(QStringLiteral("Layer"), filterLayer_);
    filterStyle_ = lineEdit(filterBox, "FilterStyle", QStringLiteral("a style, or ByLayer"));
    filterStyle_->setToolTip(QStringLiteral("The style worn; ByLayer for none") + wild);
    form->addRow(QStringLiteral("Style"), filterStyle_);
    filterColour_ = lineEdit(filterBox, "FilterColour", QStringLiteral("#RRGGBB or ByLayer"));
    form->addRow(QStringLiteral("Colour"), filterColour_);
    auto* propertyRow = new QWidget(filterBox);
    auto* propertyLayout = new QHBoxLayout(propertyRow);
    propertyLayout->setContentsMargins(0, 0, 0, 0);
    filterProperty_ = lineEdit(propertyRow, "FilterProperty", QStringLiteral("key"));
    filterValue_ = lineEdit(propertyRow, "FilterValue", QStringLiteral("value, e.g. TREE*"));
    filterValue_->setToolTip(QStringLiteral("The value as the properties panel prints it") + wild);
    propertyLayout->addWidget(filterProperty_);
    propertyLayout->addWidget(filterValue_);
    form->addRow(QStringLiteral("Property"), propertyRow);
    filterText_ = lineEdit(filterBox, "FilterText", QStringLiteral("a text's words, e.g. CH *"));
    form->addRow(QStringLiteral("Text"), filterText_);
    drawnOnly_ = new QCheckBox(QStringLiteral("Only what is drawn (not hidden)"), filterBox);
    named(drawnOnly_, "DrawnOnly");
    QObject::connect(drawnOnly_, &QCheckBox::toggled, this, [this] { changed(); });
    form->addRow(QString(), drawnOnly_);
    outer->addWidget(filterBox);

    // Last: checking it signals, and every signal reads the controls above.
    scopeSelection_->setChecked(true);
    updateEnabled();
}

void ScopeFilterWidget::changed()
{
    if (!reloading_ && onChanged) {
        onChanged();
    }
}

void ScopeFilterWidget::updateEnabled()
{
    view_->setEnabled(scopeView_->isChecked());
    onScreen_->setEnabled(scopeView_->isChecked());
    layers_->setEnabled(scopeLayers_->isChecked());
    sublayers_->setEnabled(scopeLayers_->isChecked());
}

std::vector<ScopeFilterView> ScopeFilterWidget::noWorkspaceViews()
{
    return {ScopeFilterView{katana::cad::kNoView, QStringLiteral("Whole drawing view"), nullptr,
                            std::nullopt}};
}

std::vector<ScopeFilterView> ScopeFilterWidget::openViews() const
{
    return views ? views() : noWorkspaceViews();
}

void ScopeFilterWidget::reload(const katana::entity::Model& model)
{
    reloading_ = true;
    // The layers, keeping the ticks.
    {
        const QSignalBlocker quiet(layers_);
        std::set<QString> checked;
        for (int i = 0; i < layers_->count(); ++i) {
            if (layers_->item(i)->checkState() == Qt::Checked) {
                checked.insert(layers_->item(i)->text());
            }
        }
        layers_->clear();
        for (const std::string& name : model.layers.names()) {
            auto* item = new QListWidgetItem(QString::fromStdString(name), layers_);
            item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
            item->setCheckState(checked.contains(item->text()) ? Qt::Checked : Qt::Unchecked);
        }
    }
    reloadViews();
    reloading_ = false;
}

void ScopeFilterWidget::reloadViews()
{
    const bool outer = reloading_;
    reloading_ = true;
    // The views, keeping the one chosen.
    {
        const QSignalBlocker quiet(view_);
        const QVariant kept = view_->currentData();
        view_->clear();
        // Each open view by its title and by the id a line names it by,
        // which is not the number in its title: "Plan 2 (VIEW 4)".
        for (const ScopeFilterView& open : openViews()) {
            const QString shown = open.id == katana::cad::kNoView
                                      ? open.title
                                      : QStringLiteral("%1 (VIEW %2)").arg(open.title).arg(open.id);
            view_->addItem(shown, QVariant::fromValue<quint32>(open.id));
        }
        if (const int index = view_->findData(kept); index >= 0) {
            view_->setCurrentIndex(index);
        }
    }
    updateEnabled();
    reloading_ = outer;
}

ScopeChoice ScopeFilterWidget::choice() const
{
    if (scopeView_->isChecked()) {
        return ScopeChoice::View;
    }
    if (scopeLayers_->isChecked()) {
        return ScopeChoice::Layers;
    }
    if (scopeDrawing_->isChecked()) {
        return ScopeChoice::Drawing;
    }
    return ScopeChoice::Selection;
}

void ScopeFilterWidget::setChoice(ScopeChoice choice)
{
    switch (choice) {
    case ScopeChoice::Selection:
        scopeSelection_->setChecked(true);
        break;
    case ScopeChoice::View:
        scopeView_->setChecked(true);
        break;
    case ScopeChoice::Layers:
        scopeLayers_->setChecked(true);
        break;
    case ScopeChoice::Drawing:
        scopeDrawing_->setChecked(true);
        break;
    }
}

Result<ScopeFilterView> ScopeFilterWidget::chosenView() const
{
    const std::vector<ScopeFilterView> open = openViews();
    const auto id = static_cast<katana::cad::ViewId>(view_->currentData().toUInt());
    const auto found = std::ranges::find(open, id, &ScopeFilterView::id);
    if (found == open.end()) {
        return makeError(ErrorCode::NotFound, "that view is no longer open; choose another");
    }
    return *found;
}

Result<ModifyScope> ScopeFilterWidget::scope() const
{
    ModifyScope scope;
    switch (choice()) {
    case ScopeChoice::Selection:
        scope.kind = ScopeKind::Selection;
        break;
    case ScopeChoice::Drawing:
        scope.kind = ScopeKind::Drawing;
        break;
    case ScopeChoice::Layers:
        scope.kind = ScopeKind::Layers;
        scope.sublayers = sublayers_->isChecked();
        for (int i = 0; i < layers_->count(); ++i) {
            if (layers_->item(i)->checkState() == Qt::Checked) {
                scope.layers.push_back(text(layers_->item(i)->text()));
            }
        }
        if (scope.layers.empty()) {
            return makeError(ErrorCode::InvalidArgument, "tick at least one layer to apply to");
        }
        break;
    case ScopeChoice::View: {
        scope.kind = ScopeKind::View;
        auto found = chosenView();
        if (!found) {
            return found.error();
        }
        scope.view = found->hidden;
        if (onScreen_->isChecked()) {
            if (!found->onScreen) {
                return noAreaOnScreen(*found);
            }
            scope.area = found->onScreen;
        }
        break;
    }
    }
    return scope;
}

Result<ModifyFilter> ScopeFilterWidget::filter() const
{
    ModifyFilter filter;
    for (const auto& [type, check] : types_) {
        if (check->isChecked()) {
            filter.types.insert(type);
        }
    }
    for (const QString& part : filterLayer_->text().split(QLatin1Char(','), Qt::SkipEmptyParts)) {
        if (const std::string pattern = text(part); !pattern.empty()) {
            filter.layers.push_back(pattern);
        }
    }
    if (const std::string style = text(filterStyle_->text()); !style.empty()) {
        filter.style = style;
    }
    if (const std::string typed = text(filterColour_->text()); !typed.empty()) {
        // The grammar's own reading of a colour, so the words and the
        // controls cannot take different colours.
        auto colour = katana::cad::parseColourOrByLayer(typed);
        if (!colour) {
            return makeError(ErrorCode::InvalidArgument,
                             "the filter's colour: type #RRGGBB or ByLayer", typed);
        }
        filter.colour = *colour;
    }
    if (const std::string key = text(filterProperty_->text()); !key.empty()) {
        filter.property = key;
    }
    if (const std::string value = text(filterValue_->text()); !value.empty()) {
        filter.propertyValue = value;
    }
    if (const std::string words = text(filterText_->text()); !words.empty()) {
        filter.text = words;
    }
    filter.drawnOnly = drawnOnly_->isChecked();
    return filter;
}

Result<ScopeWords> ScopeFilterWidget::words() const
{
    auto read = filter();
    if (!read) {
        return read.error();
    }
    ScopeWords words;
    words.filter = std::move(read).value();
    switch (choice()) {
    case ScopeChoice::Selection:
        words.source = ScopeSource::Selection;
        break;
    case ScopeChoice::Drawing:
        words.source = ScopeSource::Drawing;
        break;
    case ScopeChoice::Layers: {
        auto layers = scope();
        if (!layers) {
            return layers.error();
        }
        words.source = ScopeSource::Layers;
        words.layers = std::move(layers->layers);
        words.sublayers = layers->sublayers;
        break;
    }
    case ScopeChoice::View: {
        auto found = chosenView();
        if (!found) {
            return found.error();
        }
        if (found->id == katana::cad::kNoView) {
            return makeError(ErrorCode::InvalidState,
                             "no view is open to name in a line; choose another scope");
        }
        words.source = ScopeSource::View;
        words.view = found->id;
        // On screen: the view's area, which the window reads when the line
        // runs. Otherwise the area is dropped - its layers anywhere.
        if (onScreen_->isChecked()) {
            if (!found->onScreen) {
                return noAreaOnScreen(*found);
            }
        } else {
            words.extents = true;
        }
        break;
    }
    }
    return words;
}

Result<QString> ScopeFilterWidget::verbWords() const
{
    auto read = words();
    if (!read) {
        return read.error();
    }
    auto line = katana::cad::formatScopeWords(*read);
    if (!line) {
        return line.error();
    }
    return QString::fromStdString(*line);
}

} // namespace katana::qt
