// A form for one GDAL algorithm's arguments (argument_form.hpp).

#include "geo/argument_form.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QGroupBox>
#include <QLabel>
#include <QLineEdit>
#include <QSignalBlocker>
#include <QSpinBox>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <limits>
#include <utility>
#include <variant>

#include "geo/geo_dialog_support.hpp"
#include "katana/core/text.hpp"

namespace katana::qt {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// Far beyond any value a survey takes, and exact in a double: the ends of a
// spin box with no bound of GDAL's.
constexpr double kUnbounded = 1e12;

// A real as a line writes it: 2, 0.5, 1e-06 - never 2.000000.
class RealSpin final : public QDoubleSpinBox {
  public:
    using QDoubleSpinBox::QDoubleSpinBox;

  protected:
    [[nodiscard]] QString textFromValue(double value) const override
    {
        return QString::number(value, 'g', 12);
    }
};

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

bool isList(gp::ArgType type)
{
    return type == gp::ArgType::StringList || type == gp::ArgType::IntegerList ||
           type == gp::ArgType::RealList;
}

// GDAL's default as a line writes it; empty for none.
QString defaultWord(const gp::ArgSpec& arg)
{
    if (!arg.defaultValue) {
        return {};
    }
    return QString::fromStdString(gp::toString(*arg.defaultValue));
}

// The spin box's value that says "not given": below GDAL's minimum, or the
// far end when it has none.
double sentinelOf(const gp::ArgSpec& arg)
{
    return arg.min ? arg.min->value - 1.0 : -kUnbounded;
}

bool readsAsNumber(const QString& text, bool whole)
{
    const auto number = katana::core::parseFiniteDouble(text.trimmed().toStdString());
    return number && (!whole || *number == std::floor(*number));
}

QString word(const QString& name, const QString& value)
{
    return "--" + name + "=" + value;
}

} // namespace

ArgumentForm::ArgumentForm(QString prefix, QString advancedName, QWidget* parent)
    : QWidget(parent), prefix_(std::move(prefix)), advancedName_(std::move(advancedName))
{
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    auto* baseBody = new QWidget(this);
    base_ = new QFormLayout(baseBody);
    base_->setContentsMargins(0, 0, 0, 0);
    advanced_ = new QGroupBox("Advanced", this);
    advanced_->setObjectName(advancedName_);
    advanced_->setCheckable(true);
    advanced_->setChecked(false);
    advanced_->setToolTip("The arguments GDAL files as Advanced, Esoteric or Common");
    advancedBody_ = new QWidget(advanced_);
    advancedForm_ = new QFormLayout(advancedBody_);
    auto* advancedLayout = new QVBoxLayout(advanced_);
    advancedLayout->addWidget(advancedBody_);
    advancedBody_->setVisible(false);
    connect(advanced_, &QGroupBox::toggled, this, [this](bool open) { advancedBody_->setVisible(open); });
    layout->addWidget(baseBody);
    layout->addWidget(advanced_);
    layout->addStretch(1);
}

void ArgumentForm::setAlgorithm(const gp::AlgorithmSpec& spec, const std::set<std::string>& leaveOut,
                                bool pipelineStep)
{
    building_ = true;
    // Removing a row deletes its label and its control.
    entries_.clear();
    while (base_->rowCount() > 0) {
        base_->removeRow(0);
    }
    while (advancedForm_->rowCount() > 0) {
        advancedForm_->removeRow(0);
    }
    path_ = spec.info.path;
    for (const gp::ArgSpec& arg : spec.args) {
        if (arg.isDataset() || arg.isOutput || leaveOut.contains(arg.name) ||
            (pipelineStep && !arg.inPipelineStep)) {
            continue;
        }
        // What the executor refuses whatever the algorithm (--quiet) is no
        // choice to offer.
        if (!gp::checkTokens(path_, {"--" + arg.name}).ok()) {
            continue;
        }
        const QString name = QString::fromStdString(arg.name);
        const QString tip = QString::fromStdString(arg.description) +
                            (arg.required ? QString(" (required)") : QString());
        QWidget* control = nullptr;
        if (arg.type == gp::ArgType::Boolean) {
            auto* box = new QCheckBox(this);
            box->setChecked(defaultWord(arg) == "true");
            connect(box, &QCheckBox::toggled, this, [this] { changed(); });
            control = box;
        } else if (arg.type == gp::ArgType::String && !arg.choices.empty()) {
            auto* choice = new QComboBox(this);
            choice->addItem(QString());
            for (const std::string& option : arg.choices) {
                choice->addItem(QString::fromStdString(option));
            }
            choice->setCurrentText(defaultWord(arg));
            connect(choice, &QComboBox::currentTextChanged, this, [this] { changed(); });
            control = choice;
        } else if (arg.type == gp::ArgType::Integer) {
            auto* spin = new QSpinBox(this);
            const double low = std::max(sentinelOf(arg), double(std::numeric_limits<int>::min()));
            const double high = arg.max ? arg.max->value : double(std::numeric_limits<int>::max());
            spin->setRange(static_cast<int>(std::ceil(low)), static_cast<int>(std::floor(high)));
            spin->setSpecialValueText("(not given)");
            const QString fallback = defaultWord(arg);
            spin->setValue(readsAsNumber(fallback, true) ? fallback.toInt() : spin->minimum());
            connect(spin, &QSpinBox::valueChanged, this, [this] { changed(); });
            control = spin;
        } else if (arg.type == gp::ArgType::Real) {
            auto* spin = new RealSpin(this);
            spin->setDecimals(9);
            spin->setRange(sentinelOf(arg), arg.max ? arg.max->value : kUnbounded);
            spin->setSpecialValueText("(not given)");
            const QString fallback = defaultWord(arg);
            spin->setValue(readsAsNumber(fallback, false) ? fallback.toDouble() : spin->minimum());
            connect(spin, &QDoubleSpinBox::valueChanged, this, [this] { changed(); });
            control = spin;
        } else {
            auto* edit = new QLineEdit(this);
            edit->setText(defaultWord(arg));
            edit->setPlaceholderText(isList(arg.type) ? QString("a,b,c") : QString());
            connect(edit, &QLineEdit::textChanged, this, [this] { changed(); });
            control = edit;
        }
        control->setObjectName(prefix_ + name);
        control->setToolTip(tip);
        QString label = name + (arg.required ? " *" : "") + ":";
        const bool base = arg.category == "Base" || arg.required;
        (base ? base_ : advancedForm_)->addRow(label, control);
        entries_.push_back({arg, control});
    }
    advanced_->setVisible(advancedForm_->rowCount() > 0);
    building_ = false;
    updateEnabled();
}

QString ArgumentForm::text(const Entry& entry) const
{
    if (const auto* box = dynamic_cast<const QCheckBox*>(entry.control)) {
        return box->isChecked() ? "true" : "false";
    }
    if (const auto* choice = dynamic_cast<const QComboBox*>(entry.control)) {
        return choice->currentText();
    }
    if (const auto* spin = dynamic_cast<const QSpinBox*>(entry.control)) {
        return spin->value() == spin->minimum() ? QString() : QString::number(spin->value());
    }
    if (const auto* spin = dynamic_cast<const QDoubleSpinBox*>(entry.control)) {
        return spin->value() == spin->minimum() ? QString()
                                                : QString::number(spin->value(), 'g', 12);
    }
    if (const auto* edit = dynamic_cast<const QLineEdit*>(entry.control)) {
        return edit->text().trimmed();
    }
    return {};
}

QString ArgumentForm::defaultText(const Entry& entry) const
{
    if (entry.spec.type == gp::ArgType::Boolean) {
        return defaultWord(entry.spec) == "true" ? "true" : "false";
    }
    return defaultWord(entry.spec);
}

bool ArgumentForm::given(const std::string& name) const
{
    for (const Entry& entry : entries_) {
        if (entry.spec.name == name) {
            const QString value = text(entry);
            return !value.isEmpty() && value != defaultText(entry);
        }
    }
    return false;
}

std::vector<std::string> ArgumentForm::arguments() const
{
    std::vector<std::string> names;
    for (const Entry& entry : entries_) {
        names.push_back(entry.spec.name);
    }
    return names;
}

QWidget* ArgumentForm::control(const std::string& name) const
{
    for (const Entry& entry : entries_) {
        if (entry.spec.name == name) {
            return entry.control;
        }
    }
    return nullptr;
}

Result<QStringList> ArgumentForm::words() const
{
    QStringList words;
    for (const Entry& entry : entries_) {
        const gp::ArgSpec& arg = entry.spec;
        const QString name = QString::fromStdString(arg.name);
        if (!entry.control->isEnabled() || !given(arg.name)) {
            if (arg.required && entry.control->isEnabled() && !given(arg.name) &&
                defaultText(entry).isEmpty()) {
                return invalid(name + " is required: give it a value");
            }
            continue;
        }
        const QString value = text(entry);
        if (value.contains('"')) {
            return invalid(name + " holds a double quote, which a command line cannot carry");
        }
        switch (arg.type) {
        case gp::ArgType::Boolean:
            words << (value == "true" ? "--" + name : word(name, "false"));
            break;
        case gp::ArgType::Integer:
        case gp::ArgType::Real: {
            const double number = value.toDouble();
            if (arg.min && (arg.min->inclusive ? number < arg.min->value : number <= arg.min->value)) {
                return invalid(name + " must be " + (arg.min->inclusive ? "at least " : "above ") +
                               QString::number(arg.min->value, 'g', 12) + ", not " + value);
            }
            if (arg.max && (arg.max->inclusive ? number > arg.max->value : number >= arg.max->value)) {
                return invalid(name + " must be " + (arg.max->inclusive ? "at most " : "below ") +
                               QString::number(arg.max->value, 'g', 12) + ", not " + value);
            }
            words << word(name, value);
            break;
        }
        case gp::ArgType::StringList:
        case gp::ArgType::IntegerList:
        case gp::ArgType::RealList: {
            QStringList items;
            for (const QString& item : value.split(',')) {
                items << item.trimmed();
            }
            const auto count = static_cast<int>(items.size());
            if (count < arg.minCount || (arg.maxCount >= 0 && count > arg.maxCount)) {
                const QString wanted = arg.minCount == arg.maxCount
                                           ? QString::number(arg.minCount)
                                           : QString("%1 to %2").arg(arg.minCount).arg(
                                                 arg.maxCount < 0 ? QString("any number")
                                                                  : QString::number(arg.maxCount));
                return invalid(name + " takes " + wanted + " values, not " + QString::number(count));
            }
            for (const QString& item : items) {
                if (item.isEmpty() ||
                    (arg.type != gp::ArgType::StringList &&
                     !readsAsNumber(item, arg.type == gp::ArgType::IntegerList))) {
                    return invalid(name + " takes " +
                                   (arg.type == gp::ArgType::StringList
                                        ? QString("words")
                                        : arg.type == gp::ArgType::IntegerList ? QString("whole numbers")
                                                                                : QString("numbers")) +
                                   " separated by commas: '" + value + "'");
                }
            }
            // Packed lists (--size=10,20) are one word; the rest are given
            // once per item, as GDAL's own command line repeats them.
            if (arg.packedValues || !arg.repeatable) {
                auto packed = lineWord(word(name, items.join(',')), name);
                if (!packed) {
                    return packed.error();
                }
                words << *packed;
            } else {
                for (const QString& item : items) {
                    auto one = lineWord(word(name, item), name);
                    if (!one) {
                        return one.error();
                    }
                    words << *one;
                }
            }
            break;
        }
        case gp::ArgType::String: {
            auto one = lineWord(word(name, value), name);
            if (!one) {
                return one.error();
            }
            words << *one;
            break;
        }
        case gp::ArgType::Dataset:
        case gp::ArgType::DatasetList:
            break;
        }
    }
    return words;
}

bool ArgumentForm::setValue(const std::string& name, const QString& value)
{
    for (Entry& entry : entries_) {
        if (entry.spec.name != name) {
            continue;
        }
        if (auto* box = dynamic_cast<QCheckBox*>(entry.control)) {
            box->setChecked(value.isEmpty() || value.compare("true", Qt::CaseInsensitive) == 0 ||
                            value == "1" || value.compare("yes", Qt::CaseInsensitive) == 0);
            return true;
        }
        if (auto* choice = dynamic_cast<QComboBox*>(entry.control)) {
            const int index = choice->findText(value, Qt::MatchFixedString);
            if (index < 0) {
                return false;
            }
            choice->setCurrentIndex(index);
            return true;
        }
        if (auto* spin = dynamic_cast<QSpinBox*>(entry.control)) {
            if (!readsAsNumber(value, true)) {
                return false;
            }
            spin->setValue(value.toInt());
            return spin->value() == value.toInt();
        }
        if (auto* spin = dynamic_cast<QDoubleSpinBox*>(entry.control)) {
            if (!readsAsNumber(value, false)) {
                return false;
            }
            spin->setValue(value.toDouble());
            return spin->value() == value.toDouble();
        }
        if (auto* edit = dynamic_cast<QLineEdit*>(entry.control)) {
            edit->setText(value);
            return true;
        }
    }
    return false;
}

void ArgumentForm::clear()
{
    building_ = true;
    for (Entry& entry : entries_) {
        const QString fallback = defaultWord(entry.spec);
        if (auto* box = dynamic_cast<QCheckBox*>(entry.control)) {
            box->setChecked(fallback == "true");
        } else if (auto* choice = dynamic_cast<QComboBox*>(entry.control)) {
            choice->setCurrentText(fallback);
        } else if (auto* spin = dynamic_cast<QSpinBox*>(entry.control)) {
            spin->setValue(readsAsNumber(fallback, true) ? fallback.toInt() : spin->minimum());
        } else if (auto* real = dynamic_cast<QDoubleSpinBox*>(entry.control)) {
            real->setValue(readsAsNumber(fallback, false) ? fallback.toDouble() : real->minimum());
        } else if (auto* edit = dynamic_cast<QLineEdit*>(entry.control)) {
            edit->setText(fallback);
        }
    }
    building_ = false;
    changed();
}

void ArgumentForm::changed()
{
    if (building_) {
        return;
    }
    updateEnabled();
    if (onChanged) {
        onChanged();
    }
}

void ArgumentForm::updateEnabled()
{
    for (Entry& entry : entries_) {
        bool enabled = true;
        // One of an exclusion group: disabled once a rival is given.
        if (!entry.spec.exclusionGroup.empty()) {
            for (const Entry& rival : entries_) {
                if (&rival != &entry && rival.spec.exclusionGroup == entry.spec.exclusionGroup &&
                    given(rival.spec.name)) {
                    enabled = false;
                }
            }
        }
        // Dependent: enabled once what it depends on (that the form shows) is given.
        for (const std::string& other : entry.spec.dependsOn) {
            if (control(other) != nullptr && !given(other)) {
                enabled = false;
            }
        }
        entry.control->setEnabled(enabled);
    }
}

} // namespace katana::qt
