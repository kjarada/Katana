#include "customisation/code_manager_support.hpp"

#include <string_view>

#include <QColor>
#include <QComboBox>
#include <QLineEdit>
#include <QPainter>
#include <QCompleter>
#include <QPixmap>

#include "customisation/code_manager.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/entity/tables.hpp"

namespace katana::qt {

DefinitionState linestyleState(const katana::cad::Document* document, std::string_view name)
{
    if (katana::cad::isPlainLinestyle(name)) {
        return DefinitionState::Plain;
    }
    const katana::entity::LineStyle* definition =
        document != nullptr ? document->definitionFor(name) : nullptr;
    if (definition == nullptr) {
        return DefinitionState::Undefined;
    }
    return definition->atVertices ? DefinitionState::WrongKind : DefinitionState::Defined;
}

DefinitionState symbolState(const katana::cad::Document* document, std::string_view name)
{
    if (document != nullptr && document->definitionFor(name) != nullptr) {
        return DefinitionState::Defined;
    }
    return katana::entity::isBuiltInSymbolName(name) ? DefinitionState::BuiltIn
                                                     : DefinitionState::Undefined;
}

QString definitionLabel(const std::string& name, DefinitionState state)
{
    const QString shown = QString::fromStdString(name);
    switch (state) {
    case DefinitionState::Plain:
        return QObject::tr("%1 (plain line)").arg(shown);
    case DefinitionState::Defined:
        return shown;
    case DefinitionState::BuiltIn:
        return QObject::tr("%1 (built in)").arg(shown);
    case DefinitionState::WrongKind:
        return QObject::tr("%1 (a vertex symbol)").arg(shown);
    case DefinitionState::Undefined:
        return QObject::tr("%1 (not defined)").arg(shown);
    }
    return shown;
}

QIcon colourSwatch(const katana::entity::Color& colour, int size)
{
    QPixmap swatch(size, size);
    swatch.fill(Qt::transparent);
    QPainter painter(&swatch);
    painter.fillRect(QRect(1, 1, size - 2, size - 2), QColor(colour.r, colour.g, colour.b));
    painter.setPen(QColor(128, 128, 128));
    painter.drawRect(QRect(1, 1, size - 3, size - 3));
    painter.end();
    return QIcon(swatch);
}

void makeColourField(QComboBox* box)
{
    box->setEditable(true);
    box->setInsertPolicy(QComboBox::NoInsert);
    box->addItem(QString());
    // archive12d's own table, so the field offers exactly the names
    // standardColour draws. A person may still type any other name: a
    // project may define its own colours, and a map's "sui water potable" is kept
    // as written.
    for (const std::string& name : katana::archive12d::standardColourNames()) {
        const auto rgb = katana::archive12d::standardColour(name);
        box->addItem(rgb ? colourSwatch(*rgb, 14) : QIcon(), QString::fromStdString(name));
    }
    // A combo's own lookup at the end of an edit folds case and would turn a
    // typed "Blue" into the listed "blue": a map keeps names as written.
    if (box->completer() != nullptr) {
        box->completer()->setCaseSensitivity(Qt::CaseSensitive);
    }
}

void setColourField(QComboBox* box, const std::string& name)
{
    const QString shown = QString::fromStdString(name);
    const int row = box->findText(shown, Qt::MatchExactly | Qt::MatchCaseSensitive);
    if (row >= 0) {
        box->setCurrentIndex(row);
    } else {
        box->setCurrentIndex(-1);
        box->setEditText(shown);
    }
}

} // namespace katana::qt
