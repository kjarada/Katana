#include "command_reference_dialog.hpp"

#include <QApplication>
#include <QClipboard>
#include <QFontDatabase>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRegularExpression>
#include <QSplitter>
#include <QTreeWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <map>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "katana/cad/utilities/utility_verbs.hpp"
#include "geo/geo_verbs.hpp"
#include "survey_verbs.hpp"
#include "katana/interop/online_verbs.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

// Where an entry's index is kept on its tree item: the section's in
// Qt::UserRole, the entry's in the next, -1 on a section's own item.
constexpr int kSectionRole = Qt::UserRole;
constexpr int kEntryRole = Qt::UserRole + 1;

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// A command word as the helps write them: capitals and digits, as LAYER,
// PLOTSHEETS or 12DA never is but COPC is.
bool isCommandWord(const QString& word)
{
    static const QRegularExpression pattern("^[A-Z][A-Z0-9]*$");
    return pattern.match(word).hasMatch();
}

ReferenceEntry entryFor(QString line)
{
    ReferenceEntry entry;
    entry.text = line;
    if (line.startsWith("usage: ")) {
        line = line.mid(7);
    }
    const QStringList words = line.split(' ', Qt::SkipEmptyParts);
    if (words.isEmpty()) {
        return entry;
    }
    // A label is the help's first column ("Draw      POINT p ..."): a word
    // followed by two or more blanks, or one a command never is - with a
    // lower-case letter ("AnnoScale ANNOSCALE ...").
    const QString& first = words.front();
    const bool label = line.mid(first.size(), 2) == "  " || first != first.toUpper();
    if (label) {
        entry.title = first.endsWith(':') ? first.chopped(1) : first;
        // The command the group starts with, when one follows at once.
        if (words.size() > 1 && isCommandWord(words[1])) {
            entry.verb = words[1];
        }
        return entry;
    }
    // A command line: its leading command words, two at most, as "SHEET NEW"
    // or "ONLINE IMPORT".
    QStringList title;
    for (const QString& word : words) {
        if (!isCommandWord(word) || title.size() == 2) {
            break;
        }
        title << word;
    }
    entry.title = title.isEmpty() ? first : title.join(' ');
    entry.verb = title.join(' ');
    return entry;
}

// The first line of an entry's text, blanks run together: what the tree
// shows beside the title - without a group's label, which the title column
// already shows ("Layers  LAYER LIST ..." reads "LAYER LIST ...").
QString summaryOf(const ReferenceEntry& entry)
{
    QString first = entry.text.section('\n', 0, 0).simplified();
    if (entry.title != entry.verb && first.startsWith(entry.title + ' ')) {
        first = first.mid(entry.title.size() + 1);
    }
    return first;
}

} // namespace

std::vector<ReferenceEntry> referenceEntries(const QString& help)
{
    QString text = help;
    text.replace("\r\n", "\n");
    const QStringList lines = text.split('\n');
    std::vector<ReferenceEntry> entries;
    qsizetype start = 0;
    // The introduction: the first paragraph, when a blank line ends it and
    // entries follow.
    qsizetype blank = 0;
    while (blank < lines.size() && !lines[blank].trimmed().isEmpty()) {
        ++blank;
    }
    const bool more = std::any_of(lines.begin() + std::min(blank, lines.size()), lines.end(),
                                  [](const QString& line) { return !line.trimmed().isEmpty(); });
    if (blank > 0 && blank < lines.size() && more) {
        entries.push_back({"About", {}, lines.mid(0, blank).join('\n')});
        start = blank + 1;
    }
    bool open = false;
    for (qsizetype at = start; at < lines.size(); ++at) {
        const QString& line = lines[at];
        if (line.trimmed().isEmpty()) {
            open = false;
            continue;
        }
        if (line.front().isSpace() && open) {
            entries.back().text += '\n' + line;
            continue;
        }
        entries.push_back(entryFor(line.trimmed()));
        open = true;
    }
    return entries;
}

QString windowHelpText()
{
    // In the interpreter's layout, so the typed HELP reads as one text.
    // CUSTOMISE is the interpreter's verb, and the interpreter's help - which
    // the typed HELP prints first - has it, so it has no block here.
    return R"(Window    the verbs the desktop window's command line runs itself, beside the
          interpreter's. IMPORT, EXPORT, INFO <file>, REFS and COPC are katana_cli's
          too, and UTILITY is the interpreter's, run here through the utilities
          workbench; the rest are the window's alone, which katana_cli and katana_mcp
          refuse by name
Script    SCRIPT <file.kcs> [CONTINUE]   run a katana_cli script a line at a time, stopping
          at the first line refused unless CONTINUE (File > Run Script); a line starting
          with # is a note and runs nothing; several lines pasted run as a script, but
          pasted at a prompt for text (Text, Multiline Text) they are its lines
Import    IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]   a drawing, vector data, a
          raster or a point cloud, by its extension; vector data, a .12da archive or a DXF
          moves as one piece: LOCAL puts its lower-left corner at 0,0, ALONGSIDE on the
          drawing's, OFFSET by dE east and dN north (GIS > Import Vector Data, Placement)
Export    EXPORT <file>   the drawing, by extension (DXF, GeoPackage, GeoJSON, SHP, .12da)
Info      INFO <file>   describe a data file through GDAL or PDAL (INFO id: an entity)
IFC       IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES]
          [TOLERANCE <m>] [TAKECRS | KEEPCRS] | EXPORT <file.ifc> [UTILITIES <schedule.csv>]
          [SCHEMA <schema.csv>] [RULES <rules.csv>] [SPACING <m>] [NODRAWING] [NOENTITIES]
          [SELECTED] [NOALIGNMENTS] [NOSURFACES] [PREVIEW] | INFO <file.ifc> |
          IFC RULES <file.csv>   IFC 4.3 (File > Import IFC, Export IFC; docs/ifc.md)
Refs      REFS   the reference layers: rasters and point clouds
COPC      COPC <source> <destination.copc.laz>   convert a point cloud to COPC
PlotSheet PLOTSHEETS [path] [format=pdf|pdfs|png|tiff] [style=colour|grey|mono] [sheets=1,3-5]
          [dpi=n] [lineweight=f] [folder=path] [pattern=text]   plot the sheets (HELP SHEETS)
Plot      PLOT <file.pdf> [paper=A0..A4] [landscape|portrait] [fit|scale=N] [dpi=N]
          [style=colour|grey|mono] [lineweight=F] [margin=MM]   the drawing on one sheet, as
          the plan view shows it (File > Plot to PDF)
Snapshot  SNAPSHOT <file.png|.jpg|.tif> | CLIPBOARD [width=N] [height=N] [scale=F]
          [bg=theme|white|none] [view=plan|3d]   a picture of a view (File > Export View
          as Image, Edit > Copy View as Image)
View      GRID [ON|OFF] | SNAP [ON|OFF] (OSNAP) | SNAP <mode> [ON|OFF]   one of View > Snap
          Modes: Endpoint, Midpoint, Center, Intersection, Perpendicular, Tangent, Nearest,
          Grid | EXAGGERATION [factor]   elevations in the 3D and section views times 0.01
          to 1000 (View > Vertical Exaggeration); alone, the factor now. VIEWS and ZOOM are
          the interpreter's, answered by this window's views (Commands, Views and Zoom)
Online    ONLINE PROVIDERS | LAYERS | INFO | IMPORT | CUSTOM | KEY   online data (GIS >
          Online Data); the usage of each is under Online data
Utility   UTILITY REPORT|VERIFY|CLEARANCE|CHECK|DRAW|REGRADE|SCHEDULE   subsurface
          utilities, on a schedule or what is drawn (HELP UTILITY)
Quit      QUIT | EXIT   close the window; in a script, end the script
Tools     a bare tool word typed here starts the tool: LINE, L, C, TR, or an id such as
          draw.circle.ttr; with arguments it is the interpreter's (LINE 0,0 10,0), and so
          is a script's, a paste's or a dialog's line, as katana_cli runs it (ERASE there
          erases the selection). A bare LS typed starts the List tool, where katana_cli
          reads LS as LABELSTYLE)";
}

std::vector<ReferenceSection> commandReferenceSections()
{
    std::vector<ReferenceSection> sections;
    sections.push_back(
        {"Commands", referenceEntries(qs(katana::cad::CommandInterpreter::helpText()))});
    sections.push_back({"Sheets", referenceEntries(qs(katana::cad::plotting::sheetVerbHelp()))});
    sections.push_back({"Subsurface utilities",
                        referenceEntries(qs(katana::cad::utilities::utilityVerbHelp()))});
    sections.push_back({"Online data", referenceEntries(qs(katana::interop::onlineUsage()))});
    // GDAL and the geoprocessing verbs, from the executor's own table, so the
    // window and katana_help cannot list them differently.
    sections.push_back({"Geoprocessing", referenceEntries(qs(katana::app::geo::helpText()))});
    // SURVEY READ and IMPORT, from the session's own help, as katana_help says them.
    sections.push_back(
        {"Survey field files", referenceEntries(qs(katana::app::surveyHelpText()))});
    sections.push_back({"Window", referenceEntries(windowHelpText())});
    // The tools, a section a menu, in the menus' order: what each is called,
    // the words that start it, its key and what it does.
    std::map<std::string, std::vector<ReferenceEntry>> tools;
    std::vector<std::string> categories;
    for (const katana::cad::ToolInfo* tool : katana::cad::toolCatalog().all()) {
        if (!tools.contains(tool->category)) {
            categories.push_back(tool->category);
        }
        QStringList aliases;
        for (const std::string& alias : tool->aliases) {
            aliases << qs(alias);
        }
        ReferenceEntry entry;
        entry.title = qs(tool->name);
        entry.verb = aliases.isEmpty() ? qs(tool->id) : aliases.front();
        entry.text = qs(tool->name) + " (" + (aliases.isEmpty() ? qs(tool->id) : aliases.join(", ")) +
                     ")" + (tool->shortcut.empty() ? QString() : "   " + qs(tool->shortcut)) +
                     "\n          " + qs(tool->tip) + "\n          id " + qs(tool->id) + ", " +
                     qs(tool->category) + " > " + qs(tool->group);
        tools[tool->category].push_back(std::move(entry));
    }
    for (const std::string& category : categories) {
        // "Draw tools"; the menu already called Tools stays "Tools".
        const QString name = qs(category);
        sections.push_back({name.endsWith("Tools") ? name : name + " tools",
                            std::move(tools[category])});
    }
    return sections;
}

// ---- the dialog -----------------------------------------------------------------------------

CommandReferenceDialog::CommandReferenceDialog(std::vector<ReferenceSection> sections,
                                               std::function<void(const QString&)> insertCommand,
                                               QWidget* parent)
    : QDialog(parent), sections_(std::move(sections)), insertCommand_(std::move(insertCommand))
{
    setObjectName("commandReferenceDialog");
    setWindowTitle("Command Reference");
    setModal(false);
    resize(900, 620);

    search_ = new QLineEdit(this);
    search_->setObjectName("commandReferenceSearch");
    search_->setPlaceholderText("Search: words that must all occur, such as layer colour");
    search_->setClearButtonEnabled(true);

    tree_ = new QTreeWidget(this);
    tree_->setObjectName("commandReferenceTree");
    tree_->setColumnCount(2);
    tree_->setHeaderLabels({"Command", "What it takes"});
    tree_->setUniformRowHeights(true);
    tree_->header()->setSectionResizeMode(0, QHeaderView::ResizeToContents);
    tree_->setToolTip("Double-click a command to put it on the command line");

    detail_ = new QPlainTextEdit(this);
    detail_->setObjectName("commandReferenceDetail");
    detail_->setReadOnly(true);
    detail_->setFont(katana::qt::theme::monospaceFont());
    detail_->setLineWrapMode(QPlainTextEdit::NoWrap);
    detail_->setPlaceholderText("Choose a command to see all it takes.");

    auto* split = new QSplitter(Qt::Vertical, this);
    split->addWidget(tree_);
    split->addWidget(detail_);
    split->setStretchFactor(0, 3);
    split->setStretchFactor(1, 1);

    count_ = new QLabel(this);
    count_->setObjectName("commandReferenceCount");
    auto* copy = new QPushButton("Copy", this);
    copy->setObjectName("commandReferenceCopy");
    copy->setToolTip("Copy the chosen command, or every command shown");
    auto* close = new QPushButton("Close", this);
    close->setObjectName("commandReferenceClose");
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(count_, 1);
    buttons->addWidget(copy);
    buttons->addWidget(close);

    auto* layout = new QVBoxLayout(this);
    layout->addWidget(search_);
    layout->addWidget(split, 1);
    layout->addLayout(buttons);

    for (std::size_t s = 0; s < sections_.size(); ++s) {
        auto* section = new QTreeWidgetItem(tree_, {sections_[s].name});
        section->setData(0, kSectionRole, static_cast<int>(s));
        section->setData(0, kEntryRole, -1);
        QFont bold = section->font(0);
        bold.setBold(true);
        section->setFont(0, bold);
        for (std::size_t e = 0; e < sections_[s].entries.size(); ++e) {
            const ReferenceEntry& entry = sections_[s].entries[e];
            auto* item = new QTreeWidgetItem(section, {entry.title, summaryOf(entry)});
            item->setData(0, kSectionRole, static_cast<int>(s));
            item->setData(0, kEntryRole, static_cast<int>(e));
            item->setToolTip(1, entry.text);
        }
    }

    connect(search_, &QLineEdit::textChanged, this, [this](const QString& text) { setFilter(text); });
    connect(tree_, &QTreeWidget::currentItemChanged, this, [this] { showDetail(); });
    connect(tree_, &QTreeWidget::itemDoubleClicked, this, [this](QTreeWidgetItem* item, int) {
        const int e = item->data(0, kEntryRole).toInt();
        if (e < 0 || !insertCommand_) {
            return;
        }
        const ReferenceEntry& entry =
            sections_[static_cast<std::size_t>(item->data(0, kSectionRole).toInt())]
                .entries[static_cast<std::size_t>(e)];
        if (!entry.verb.isEmpty()) {
            insertCommand_(entry.verb + ' ');
        }
    });
    connect(copy, &QPushButton::clicked, this,
            [this] { QApplication::clipboard()->setText(copyText()); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    setFilter({});
}

void CommandReferenceDialog::setFilter(const QString& text)
{
    if (search_->text() != text) {
        search_->setText(text); // comes back here through textChanged
        return;
    }
    const QStringList words = text.split(' ', Qt::SkipEmptyParts);
    int shown = 0;
    int total = 0;
    for (int s = 0; s < tree_->topLevelItemCount(); ++s) {
        QTreeWidgetItem* section = tree_->topLevelItem(s);
        const ReferenceSection& source = sections_[static_cast<std::size_t>(s)];
        int visible = 0;
        for (int e = 0; e < section->childCount(); ++e) {
            const ReferenceEntry& entry = source.entries[static_cast<std::size_t>(e)];
            const QString haystack = source.name + ' ' + entry.title + ' ' + entry.text;
            const bool match = std::ranges::all_of(words, [&haystack](const QString& word) {
                return haystack.contains(word, Qt::CaseInsensitive);
            });
            section->child(e)->setHidden(!match);
            visible += match ? 1 : 0;
        }
        section->setHidden(visible == 0);
        section->setText(1, QString("%1 of %2").arg(visible).arg(section->childCount()));
        // A search opens what it found; the whole reference opens closed, to
        // be read by its sections.
        section->setExpanded(!words.isEmpty() && visible > 0);
        shown += visible;
        total += section->childCount();
    }
    count_->setText(words.isEmpty() ? QString("%1 entries").arg(total)
                                    : QString("%1 of %2 entries").arg(shown).arg(total));
}

bool CommandReferenceDialog::showSection(const QString& name)
{
    for (int s = 0; s < tree_->topLevelItemCount(); ++s) {
        QTreeWidgetItem* section = tree_->topLevelItem(s);
        if (section->text(0) != name) {
            continue;
        }
        setFilter({});
        section->setExpanded(true);
        tree_->setCurrentItem(section->childCount() > 0 ? section->child(0) : section);
        tree_->scrollToItem(section, QAbstractItemView::PositionAtTop);
        return true;
    }
    return false;
}

int CommandReferenceDialog::shownEntries() const
{
    int shown = 0;
    for (int s = 0; s < tree_->topLevelItemCount(); ++s) {
        const QTreeWidgetItem* section = tree_->topLevelItem(s);
        for (int e = 0; e < section->childCount(); ++e) {
            shown += section->child(e)->isHidden() ? 0 : 1;
        }
    }
    return shown;
}

QString CommandReferenceDialog::copyText() const
{
    if (const QTreeWidgetItem* item = tree_->currentItem();
        item != nullptr && !item->isHidden() && item->data(0, kEntryRole).toInt() >= 0) {
        return sections_[static_cast<std::size_t>(item->data(0, kSectionRole).toInt())]
            .entries[static_cast<std::size_t>(item->data(0, kEntryRole).toInt())]
            .text;
    }
    QStringList shown;
    for (int s = 0; s < tree_->topLevelItemCount(); ++s) {
        const QTreeWidgetItem* section = tree_->topLevelItem(s);
        for (int e = 0; e < section->childCount(); ++e) {
            if (!section->child(e)->isHidden()) {
                shown << sections_[static_cast<std::size_t>(s)]
                             .entries[static_cast<std::size_t>(e)]
                             .text;
            }
        }
    }
    return shown.join('\n');
}

void CommandReferenceDialog::showDetail()
{
    const QTreeWidgetItem* item = tree_->currentItem();
    if (item == nullptr || item->data(0, kEntryRole).toInt() < 0) {
        detail_->clear();
        return;
    }
    detail_->setPlainText(sections_[static_cast<std::size_t>(item->data(0, kSectionRole).toInt())]
                              .entries[static_cast<std::size_t>(item->data(0, kEntryRole).toInt())]
                              .text);
}

} // namespace katana::qt
