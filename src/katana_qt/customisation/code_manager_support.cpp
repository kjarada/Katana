#include "customisation/code_manager_support.hpp"

#include <fstream>
#include <functional>
#include <set>
#include <string_view>
#include <utility>

#include <QColor>
#include <QComboBox>
#include <QCompleter>
#include <QLineEdit>
#include <QPainter>
#include <QPixmap>
#include <QSignalBlocker>

#include "customisation/code_manager.hpp"
#include "katana/cad/colour_lookup.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/colour_names.hpp"
#include "katana/entity/customisation.hpp"
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
        return QObject::tr("%1 (an `at vertices` symbol)").arg(shown);
    case DefinitionState::Undefined:
        return QObject::tr("%1 (not defined)").arg(shown);
    }
    return shown;
}

std::optional<katana::entity::Color> colourOf(const katana::cad::Document* document,
                                              std::string_view name)
{
    return document != nullptr ? katana::cad::resolveColour(*document, name)
                               : katana::entity::standardColour(name);
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

namespace {

constexpr int kSwatchPixels = 14;

// The names a colour field offers: none, then the customisation's own - the
// names only this session knows, which is why they come first - then the
// standard ones. Both from the tables that resolve them, so the field offers
// exactly what is drawn and keeps no copy of either. A person may still type
// any other name: a rule's "sui water potable" is kept as written whether or
// not this session defines it.
void listColours(QComboBox* box, const katana::cad::Document* document)
{
    box->addItem(QString());
    if (document != nullptr) {
        for (const katana::entity::ColourTable::Entry& entry :
             document->customisationState().colours.entries()) {
            box->addItem(colourSwatch(entry.colour, kSwatchPixels),
                         QString::fromStdString(entry.name));
        }
    }
    for (const std::string& name : katana::entity::standardColourNames()) {
        const auto rgb = katana::entity::standardColour(name);
        box->addItem(rgb ? colourSwatch(*rgb, kSwatchPixels) : QIcon(),
                     QString::fromStdString(name));
    }
}

} // namespace

void makeColourField(QComboBox* box, const katana::cad::Document* document)
{
    box->setEditable(true);
    box->setInsertPolicy(QComboBox::NoInsert);
    listColours(box, document);
    // A combo's own lookup at the end of an edit folds case and would turn a
    // typed "Blue" into the listed "blue": a map keeps names as written.
    if (box->completer() != nullptr) {
        box->completer()->setCaseSensitivity(Qt::CaseSensitive);
    }
}

void refreshColourField(QComboBox* box, const katana::cad::Document* document)
{
    const std::string shown = box->currentText().toStdString();
    // Neither the box nor the field inside it says anything while the list
    // is emptied and filled: the name shown is the same before and after,
    // and a listener would otherwise read a rule with no colour in between.
    const QSignalBlocker quietBox(box);
    const QSignalBlocker quietField(box->lineEdit());
    box->clear();
    listColours(box, document);
    setColourField(box, shown);
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

QString customisationFileFilter()
{
    return QObject::tr("Katana customisation (*.customisation.json *.json)");
}

std::string exportedCustomisationName(const katana::cad::Document* document,
                                      const std::filesystem::path& file)
{
    if (document != nullptr && !document->customisationState().name.empty()) {
        return document->customisationState().name;
    }
    std::string name = katana::core::pathToUtf8(file.filename());
    for (const std::string_view suffix : {std::string_view(".json"),
                                          std::string_view(".customisation")}) {
        if (name.size() > suffix.size() && katana::core::lowered(name).ends_with(suffix)) {
            name.erase(name.size() - suffix.size());
        }
    }
    return katana::entity::validateCustomisationName(name).ok() ? name
                                                                : std::string("customisation");
}

std::vector<katana::entity::CustomisationSourceNote>
sourcesOfPart(std::vector<katana::entity::CustomisationSourceNote> sources,
              CustomisationPart part, bool withColours)
{
    std::vector<katana::entity::CustomisationSourceNote> standing;
    standing.reserve(sources.size());
    for (katana::entity::CustomisationSourceNote& source : sources) {
        const bool brought =
            part == CustomisationPart::Codes ? source.rules : source.definitions;
        const bool neither = !source.definitions && !source.rules;
        if (!brought && !(neither && withColours)) {
            continue;
        }
        source.definitions = brought && part == CustomisationPart::Definitions;
        source.rules = brought && part == CustomisationPart::Codes;
        standing.push_back(std::move(source));
    }
    return standing;
}

katana::entity::Customisation exportedPart(katana::entity::Customisation session,
                                           CustomisationPart part,
                                           const std::vector<std::string>& only)
{
    // The name, the description, the notice and what it is based on stay as
    // the session has them: everything below takes something OUT.
    katana::entity::Customisation written = std::move(session);
    if (part == CustomisationPart::Codes) {
        written.library = {};
    } else {
        written.map = {};
        if (!only.empty()) {
            katana::entity::StyleLibrary chosen;
            for (const std::string& name : only) {
                if (const katana::entity::LineStyle* definition = written.library.find(name)) {
                    // Refused only for a name given twice, which is then
                    // there once: the definition came out of a library.
                    (void)chosen.add(*definition);
                }
            }
            written.library = std::move(chosen);
        }
    }
    written.linework.reset();
    written.automation.reset();

    // The colours what is left NAMES, by the fold a name is found by: the
    // three places a rule names one (the lint's own three,
    // cad::lintSurveyRule) and a definition's pens.
    std::set<std::string, std::less<>> named;
    const auto names = [&](const std::string& colour) {
        if (!colour.empty()) {
            named.insert(katana::entity::foldColourName(colour));
        }
    };
    for (const katana::entity::SurveyRule& rule : written.map.rules()) {
        names(rule.colour);
        if (rule.symbol) {
            names(rule.symbol->colour);
        }
        if (rule.textStyle) {
            names(rule.textStyle->colour);
        }
    }
    written.library.forEach([&](const katana::entity::LineStyle& definition) {
        for (const katana::entity::Stroke& stroke : definition.strokes) {
            if (stroke.op == katana::entity::StrokeOp::Pen) {
                names(stroke.pen);
            }
        }
    });
    katana::entity::ColourTable carried;
    for (const katana::entity::ColourTable::Entry& colour : written.colours.entries()) {
        if (named.contains(katana::entity::foldColourName(colour.name))) {
            // Cannot be refused: the entry comes out of a table that took it.
            (void)carried.add(colour.name, colour.colour);
        }
    }
    written.colours = std::move(carried);
    written.sources =
        sourcesOfPart(std::move(written.sources), part, !written.colours.empty());
    return written;
}

katana::core::Status writeFileBytes(const std::filesystem::path& path, std::string_view bytes)
{
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    if (!out) {
        return katana::core::makeError(katana::core::ErrorCode::FileExportFailure,
                                       "cannot open the file for writing",
                                       katana::core::pathToUtf8(path));
    }
    out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    out.close();
    if (!out) {
        return katana::core::makeError(katana::core::ErrorCode::FileExportFailure,
                                       "the file could not be written",
                                       katana::core::pathToUtf8(path));
    }
    return {};
}

} // namespace katana::qt
