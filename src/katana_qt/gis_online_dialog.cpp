#include "gis_online_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDate>
#include <QDateEdit>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSplitter>
#include <QTreeWidget>
#include <QTreeWidgetItemIterator>
#include <QVBoxLayout>

#include <algorithm>
#include <functional>
#include <map>

#include "katana/gis/web_access.hpp"
#include "katana/interop/online_requests.hpp"
#include "theme.hpp"

namespace katana::qt {
namespace {

namespace interop = katana::interop;

constexpr int kProviderRole = Qt::UserRole;
constexpr int kLayerRole = Qt::UserRole + 1;

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// The kind as a person reads it.
QString kindText(interop::OnlineLayerKind kind)
{
    switch (kind) {
    case interop::OnlineLayerKind::Imagery:
        return "Raster imagery";
    case interop::OnlineLayerKind::Elevation:
        return "Raster elevation";
    case interop::OnlineLayerKind::Vector:
        return "Vector features";
    case interop::OnlineLayerKind::Catalogue:
        return "Catalogue search";
    }
    return {};
}

// "Australia/NSW" -> ("Australia", "NSW"); "Global" -> ("Global", "").
std::pair<QString, QString> splitGroup(const std::string& group)
{
    const QString text = qs(group);
    const qsizetype slash = text.indexOf('/');
    return slash < 0 ? std::pair{text, QString()} : std::pair{text.left(slash), text.mid(slash + 1)};
}

} // namespace

OnlineDataDialog::OnlineDataDialog(OnlineDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("onlineDataDialog");
    setWindowTitle("Online Data");
    setModal(false);
    resize(980, 640);

    search_ = new QLineEdit(this);
    search_->setObjectName("onlineSearch");
    search_->setPlaceholderText("Search providers and layers");
    search_->setClearButtonEnabled(true);
    tree_ = new QTreeWidget(this);
    tree_->setObjectName("onlineProviders");
    tree_->setHeaderLabels({"Provider / layer", "Kind"});
    tree_->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    tree_->setUniformRowHeights(true);

    auto* left = new QWidget(this);
    auto* leftLayout = new QVBoxLayout(left);
    leftLayout->setContentsMargins(0, 0, 0, 0);
    leftLayout->addWidget(search_);
    leftLayout->addWidget(tree_, 1);
    customUrl_ = new QLineEdit(left);
    customUrl_->setObjectName("onlineCustomUrl");
    customUrl_->setPlaceholderText("https://... a WMS, WMTS, WFS, WCS, ArcGIS REST, OGC API, STAC or COG address");
    addCustom_ = new QPushButton("Add Custom Service...", left);
    addCustom_->setObjectName("onlineAddCustom");
    addCustom_->setToolTip("Ask the service at this address what it offers, and add its layers "
                           "under Custom");
    auto* customRow = new QHBoxLayout;
    customRow->addWidget(customUrl_, 1);
    customRow->addWidget(addCustom_);
    leftLayout->addLayout(customRow);

    details_ = new QLabel(this);
    details_->setObjectName("onlineDetails");
    details_->setWordWrap(true);
    details_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    details_->setAlignment(Qt::AlignTop | Qt::AlignLeft);
    details_->setMinimumHeight(170);

    // Where.
    area_ = new QComboBox(this);
    area_->setObjectName("onlineArea");
    area_->addItem("Current view", static_cast<int>(interop::OnlineAreaKind::View));
    area_->addItem("Drawing extents", static_cast<int>(interop::OnlineAreaKind::Drawing));
    area_->addItem("Selection", static_cast<int>(interop::OnlineAreaKind::Selection));
    area_->addItem("Typed box", static_cast<int>(interop::OnlineAreaKind::Box));
    box_ = new QLineEdit(this);
    box_->setObjectName("onlineBox");
    box_->setPlaceholderText("x0,y0,x1,y1");
    boxCrs_ = new QComboBox(this);
    boxCrs_->setObjectName("onlineBoxCrs");
    boxCrs_->addItem("Project coordinates");
    boxCrs_->addItem("WGS 84 longitude, latitude");
    auto* boxRow = new QHBoxLayout;
    boxRow->addWidget(box_, 1);
    boxRow->addWidget(boxCrs_);
    const auto boxEnabled = [this] {
        const bool typed = area_->currentData().toInt() == static_cast<int>(interop::OnlineAreaKind::Box);
        box_->setEnabled(typed);
        boxCrs_->setEnabled(typed);
    };
    connect(area_, &QComboBox::currentIndexChanged, this, boxEnabled);
    boxEnabled();

    // How.
    resolutionAuto_ = new QCheckBox("Automatic", this);
    resolutionAuto_->setObjectName("onlineResolutionAuto");
    resolutionAuto_->setChecked(true);
    resolution_ = new QDoubleSpinBox(this);
    resolution_->setObjectName("onlineResolution");
    resolution_->setDecimals(3);
    resolution_->setRange(0.01, 100000.0);
    resolution_->setValue(1.0);
    resolution_->setSuffix(" per pixel");
    resolution_->setEnabled(false);
    connect(resolutionAuto_, &QCheckBox::toggled, resolution_, [this](bool automatic) {
        resolution_->setEnabled(!automatic);
    });
    auto* resolutionRow = new QHBoxLayout;
    resolutionRow->addWidget(resolutionAuto_);
    resolutionRow->addWidget(resolution_, 1);
    targetLayer_ = new QLineEdit(this);
    targetLayer_->setObjectName("onlineTargetLayer");
    targetLayer_->setPlaceholderText("online/<provider>/<layer>");
    tag_ = new QLineEdit(this);
    tag_->setObjectName("onlineTag");
    tag_->setPlaceholderText("the layer's own, or e.g. building, highway=primary");
    useDates_ = new QCheckBox("Between", this);
    useDates_->setObjectName("onlineUseDates");
    from_ = new QDateEdit(QDate::currentDate().addDays(-90), this);
    from_->setObjectName("onlineFrom");
    from_->setDisplayFormat("yyyy-MM-dd");
    from_->setCalendarPopup(true);
    to_ = new QDateEdit(QDate::currentDate(), this);
    to_->setObjectName("onlineTo");
    to_->setDisplayFormat("yyyy-MM-dd");
    to_->setCalendarPopup(true);
    auto* datesRow = new QHBoxLayout;
    datesRow->addWidget(useDates_);
    datesRow->addWidget(from_);
    datesRow->addWidget(new QLabel("and", this));
    datesRow->addWidget(to_);
    cloud_ = new QDoubleSpinBox(this);
    cloud_->setObjectName("onlineCloud");
    cloud_->setRange(0.0, 100.0);
    cloud_->setValue(20.0);
    cloud_->setSuffix(" % cloud at most");

    projectCrs_ = new QLabel(this);
    projectCrs_->setObjectName("onlineProjectCrs");
    crs_ = new QLineEdit(this);
    crs_->setObjectName("onlineCrs");
    crs_->setPlaceholderText("EPSG:7856 - used, and set on the project, when it has none");

    key_ = new QLineEdit(this);
    key_->setObjectName("onlineKey");
    key_->setEchoMode(QLineEdit::Password);
    key_->setPlaceholderText("kept in Katana's settings, never in the project");
    saveKey_ = new QPushButton("Save Key", this);
    saveKey_->setObjectName("onlineSaveKey");
    auto* keyRow = new QHBoxLayout;
    keyRow->addWidget(key_, 1);
    keyRow->addWidget(saveKey_);

    auto* form = new QFormLayout;
    form->addRow("Area:", area_);
    form->addRow("Box:", boxRow);
    form->addRow("Resolution:", resolutionRow);
    form->addRow("Target layer:", targetLayer_);
    form->addRow("OpenStreetMap tag:", tag_);
    form->addRow("Scene dates:", datesRow);
    form->addRow("Cloud:", cloud_);
    form->addRow("Project CRS:", projectCrs_);
    form->addRow("Use CRS:", crs_);
    form->addRow("Key:", keyRow);

    import_ = new QPushButton("Import", this);
    import_->setObjectName("onlineImport");
    import_->setDefault(true);
    auto* close = new QPushButton("Close", this);
    close->setObjectName("onlineClose");
    status_ = new QLabel(this);
    status_->setObjectName("onlineStatus");
    status_->setWordWrap(true);
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(status_, 1);
    buttons->addWidget(import_);
    buttons->addWidget(close);

    auto* right = new QWidget(this);
    auto* rightLayout = new QVBoxLayout(right);
    rightLayout->setContentsMargins(0, 0, 0, 0);
    rightLayout->addWidget(details_);
    rightLayout->addLayout(form);
    rightLayout->addStretch(1);

    auto* splitter = new QSplitter(this);
    splitter->addWidget(left);
    splitter->addWidget(right);
    splitter->setStretchFactor(0, 1);
    splitter->setStretchFactor(1, 1);
    // Room for a layer's title beside its kind.
    splitter->setSizes({470, 510});
    tree_->setColumnWidth(1, 120);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(splitter, 1);
    layout->addLayout(buttons);

    connect(search_, &QLineEdit::textChanged, this, [this](const QString& text) { applyFilter(text); });
    connect(tree_, &QTreeWidget::currentItemChanged, this, [this] { showDetails(); });
    connect(import_, &QPushButton::clicked, this, [this] { importChosen(); });
    connect(addCustom_, &QPushButton::clicked, this, [this] { addCustomService(); });
    connect(saveKey_, &QPushButton::clicked, this, [this] { saveKey(); });
    connect(close, &QPushButton::clicked, this, &QDialog::close);

    refreshCatalogue();
}

void OnlineDataDialog::refreshCatalogue()
{
    std::string keepProvider;
    std::string keepLayer;
    if (const QTreeWidgetItem* current = tree_->currentItem()) {
        keepProvider = current->data(0, kProviderRole).toString().toStdString();
        keepLayer = current->data(0, kLayerRole).toString().toStdString();
    }
    tree_->clear();
    std::map<QString, QTreeWidgetItem*> tops;
    std::map<QString, QTreeWidgetItem*> groups;
    // Australia first, then Global, then the person's own.
    for (const QString& name : {QString("Australia"), QString("Global"), QString("Custom")}) {
        auto* top = new QTreeWidgetItem(tree_, {name});
        top->setFlags(Qt::ItemIsEnabled);
        tops[name] = top;
    }
    for (const interop::OnlineProvider& provider : context_.catalogue().providers) {
        const auto [topName, subName] = splitGroup(provider.userDefined ? "Custom" : provider.group);
        QTreeWidgetItem*& top = tops[topName];
        if (top == nullptr) {
            top = new QTreeWidgetItem(tree_, {topName});
            top->setFlags(Qt::ItemIsEnabled);
        }
        QTreeWidgetItem* parent = top;
        if (!subName.isEmpty()) {
            QTreeWidgetItem*& group = groups[topName + "/" + subName];
            if (group == nullptr) {
                group = new QTreeWidgetItem(top, {subName});
                group->setFlags(Qt::ItemIsEnabled);
            }
            parent = group;
        }
        auto* providerItem = new QTreeWidgetItem(parent, {qs(provider.title)});
        providerItem->setData(0, kProviderRole, qs(provider.id));
        providerItem->setToolTip(0, qs(provider.id));
        for (const interop::OnlineLayer* layer : provider.layers()) {
            auto* item = new QTreeWidgetItem(providerItem, {qs(layer->title), kindText(layer->kind)});
            item->setData(0, kProviderRole, qs(provider.id));
            item->setData(0, kLayerRole, qs(layer->id));
            item->setToolTip(0, qs(provider.id + " " + layer->id));
        }
    }
    for (auto& [name, top] : tops) {
        top->setHidden(top->childCount() == 0);
    }
    tree_->expandToDepth(1);
    applyFilter(search_->text());
    if (!keepLayer.empty()) {
        selectLayer(keepProvider, keepLayer);
    }
    const std::string crs = context_.projectCrs ? context_.projectCrs() : std::string();
    projectCrs_->setText(crs.empty() ? "none set - give one below, or set the project's"
                                     : qs(crs));
    showDetails();
}

bool OnlineDataDialog::selectLayer(const std::string& provider, const std::string& layer)
{
    for (QTreeWidgetItemIterator it(tree_); *it != nullptr; ++it) {
        if ((*it)->data(0, kProviderRole).toString().compare(qs(provider), Qt::CaseInsensitive) == 0 &&
            (*it)->data(0, kLayerRole).toString().compare(qs(layer), Qt::CaseInsensitive) == 0) {
            tree_->setCurrentItem(*it);
            tree_->scrollToItem(*it);
            return true;
        }
    }
    return false;
}

const interop::OnlineLayer* OnlineDataDialog::chosenLayer() const
{
    const QTreeWidgetItem* current = tree_->currentItem();
    if (current == nullptr || current->data(0, kLayerRole).toString().isEmpty()) {
        return nullptr;
    }
    const interop::OnlineProvider* provider = context_.catalogue().findProvider(
        current->data(0, kProviderRole).toString().toStdString());
    return provider != nullptr ? provider->findLayer(current->data(0, kLayerRole).toString().toStdString())
                               : nullptr;
}

void OnlineDataDialog::showDetails()
{
    const interop::OnlineLayer* layer = chosenLayer();
    import_->setEnabled(layer != nullptr && layer->kind != interop::OnlineLayerKind::Catalogue);
    if (layer == nullptr) {
        details_->setText("Choose a layer to see what it is, where it comes from and on what "
                          "terms.");
        for (QWidget* widget : std::initializer_list<QWidget*>{
                 key_, saveKey_, tag_, useDates_, from_, to_, cloud_, resolutionAuto_, resolution_,
                 targetLayer_}) {
            widget->setEnabled(false);
        }
        return;
    }
    const auto row = [](const QString& name, const QString& value) {
        return "<tr><td style='padding-right:10px'><b>" + name + "</b></td><td>" +
               value.toHtmlEscaped() + "</td></tr>";
    };
    const auto coverage = QString("%1, %2 to %3, %4")
                              .arg(layer->coverage[0])
                              .arg(layer->coverage[1])
                              .arg(layer->coverage[2])
                              .arg(layer->coverage[3]);
    const bool keyStored = context_.hasKey && context_.hasKey(layer->keyName);
    QString html = "<h3>" + qs(layer->title).toHtmlEscaped() + "</h3><table>";
    html += row("Provider", qs(layer->providerTitle + " (" + layer->providerId + ")"));
    html += row("Id", qs(layer->providerId + " " + layer->id));
    html += row("Kind", kindText(layer->kind));
    html += row("Service", qs(interop::toString(layer->type)) +
                               (layer->layerName.empty() ? QString() : " - " + qs(layer->layerName)));
    html += row("Address", qs(katana::gis::redactUrl(layer->endpoint)));
    html += row("Coverage", coverage + " (WGS 84)");
    if (layer->resolution) {
        html += row("Resolution", QString::number(*layer->resolution) + " m");
    }
    html += row("Licence", qs(layer->licence));
    html += row("Attribution", qs(layer->attribution));
    html += row("Key", layer->keyRequired ? (keyStored ? "needed - stored" : "needed - not stored")
                                          : QString("not needed"));
    html += row("Checked", layer->verified.empty() ? "documented, not yet seen to answer: " +
                                                         qs(layer->evidence)
                                                   : "answered " + qs(layer->verified));
    html += "</table>";
    if (layer->kind == interop::OnlineLayerKind::Catalogue) {
        html += "<p>This is a search, not data: type <tt>ONLINE LAYERS " +
                qs(layer->providerId).toHtmlEscaped() +
                " &lt;words&gt;</tt> on the command line, then add a service it finds with Add "
                "Custom Service.</p>";
    }
    details_->setText(html);
    const bool usesTag = layer->type == interop::OnlineServiceType::Overpass;
    const bool usesDates = layer->type == interop::OnlineServiceType::Stac;
    tag_->setEnabled(usesTag);
    useDates_->setEnabled(usesDates);
    from_->setEnabled(usesDates);
    to_->setEnabled(usesDates);
    cloud_->setEnabled(usesDates);
    const bool raster = layer->kind == interop::OnlineLayerKind::Imagery ||
                        layer->kind == interop::OnlineLayerKind::Elevation;
    resolutionAuto_->setEnabled(raster);
    resolution_->setEnabled(raster && !resolutionAuto_->isChecked());
    targetLayer_->setEnabled(layer->kind == interop::OnlineLayerKind::Vector);
    key_->setEnabled(true);
    saveKey_->setEnabled(true);
}

void OnlineDataDialog::applyFilter(const QString& text)
{
    const QStringList words = text.toLower().split(' ', Qt::SkipEmptyParts);
    // A layer shows when every word is in its own text or its provider's;
    // a group or provider shows when anything under it does.
    const std::function<bool(QTreeWidgetItem*, const QString&)> visit =
        [&](QTreeWidgetItem* item, const QString& inherited) {
            const QString here = inherited + " " + item->text(0).toLower() + " " +
                                 item->text(1).toLower() + " " +
                                 item->data(0, kProviderRole).toString().toLower() + " " +
                                 item->data(0, kLayerRole).toString().toLower();
            const bool isLayer = !item->data(0, kLayerRole).toString().isEmpty();
            if (isLayer) {
                const bool shown = std::all_of(words.begin(), words.end(),
                                               [&](const QString& word) { return here.contains(word); });
                item->setHidden(!shown);
                return shown;
            }
            bool any = false;
            for (int i = 0; i < item->childCount(); ++i) {
                any = visit(item->child(i), here) || any;
            }
            item->setHidden(!any);
            if (any && !words.isEmpty()) {
                item->setExpanded(true);
            }
            return any;
        };
    for (int i = 0; i < tree_->topLevelItemCount(); ++i) {
        visit(tree_->topLevelItem(i), QString());
    }
    // A search that hides the chosen layer chooses the first it left, so the
    // details always describe something the list shows.
    QTreeWidgetItem* current = tree_->currentItem();
    if (!words.isEmpty() && (current == nullptr || current->isHidden() ||
                             current->data(0, kLayerRole).toString().isEmpty())) {
        for (QTreeWidgetItemIterator it(tree_); *it != nullptr; ++it) {
            bool shown = !(*it)->isHidden();
            for (QTreeWidgetItem* parent = (*it)->parent(); shown && parent != nullptr; parent = parent->parent()) {
                shown = !parent->isHidden();
            }
            if (shown && !(*it)->data(0, kLayerRole).toString().isEmpty()) {
                tree_->setCurrentItem(*it);
                break;
            }
        }
    }
}

katana::core::Result<interop::OnlineCommand> OnlineDataDialog::command() const
{
    const interop::OnlineLayer* layer = chosenLayer();
    if (layer == nullptr) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                       "choose a layer in the list first");
    }
    // Written as the verb would be typed, and read by the verb's own parser,
    // so the dialog cannot mean anything the command line cannot.
    QStringList line{"ONLINE", "IMPORT", qs(layer->providerId), qs(layer->id)};
    const auto kind = static_cast<interop::OnlineAreaKind>(area_->currentData().toInt());
    switch (kind) {
    case interop::OnlineAreaKind::View:
        line << "area=view";
        break;
    case interop::OnlineAreaKind::Drawing:
        line << "area=drawing";
        break;
    case interop::OnlineAreaKind::Selection:
        line << "area=selection";
        break;
    case interop::OnlineAreaKind::Box:
        line << "area=" + QString(boxCrs_->currentIndex() == 1 ? "lonlat:" : "") +
                    box_->text().remove(' ');
        break;
    }
    if (resolutionAuto_->isEnabled() && !resolutionAuto_->isChecked()) {
        line << "res=" + QString::number(resolution_->value(), 'g', 12);
    }
    if (targetLayer_->isEnabled() && !targetLayer_->text().trimmed().isEmpty()) {
        line << "layer=\"" + targetLayer_->text().trimmed().remove('"') + "\"";
    }
    if (tag_->isEnabled() && !tag_->text().trimmed().isEmpty()) {
        line << "tag=\"" + tag_->text().trimmed().remove('"') + "\"";
    }
    if (useDates_->isEnabled() && useDates_->isChecked()) {
        line << "from=" + from_->date().toString("yyyy-MM-dd")
             << "to=" + to_->date().toString("yyyy-MM-dd");
    }
    if (cloud_->isEnabled()) {
        line << "cloud=" + QString::number(cloud_->value());
    }
    if (!crs_->text().trimmed().isEmpty()) {
        line << "crs=" + crs_->text().trimmed().remove(' ');
    }
    return interop::parseOnlineCommand(line.join(' ').toStdString());
}

void OnlineDataDialog::importChosen()
{
    auto parsed = command();
    if (!parsed) {
        setStatus(qs(parsed.error().message) +
                      (parsed.error().context.empty() ? QString() : " (" + qs(parsed.error().context) + ")"),
                  true);
        return;
    }
    if (!context_.run) {
        setStatus("nothing here can run an import", true);
        return;
    }
    // Said first: a run that finishes at once (a headless session waits for
    // it) reports its outcome here, which this must not then overwrite.
    setStatus("Importing " + qs(parsed->provider + " " + parsed->layer) +
              " in the background - the status bar shows its progress and can cancel it.");
    if (const auto started = context_.run(*parsed); !started) {
        setStatus(qs(started.error().describe()), true);
    }
}

void OnlineDataDialog::addCustomService()
{
    const std::string url = customUrl_->text().trimmed().toStdString();
    if (url.empty()) {
        setStatus("Type the service's address beside Add Custom Service first.", true);
        return;
    }
    if (!context_.addCustom) {
        return;
    }
    // Said first, for the reason importChosen gives.
    setStatus("Asking " + qs(katana::gis::redactUrl(url)) + " what it offers...");
    if (const auto started = context_.addCustom(url); !started) {
        setStatus(qs(started.error().describe()), true);
    }
}

void OnlineDataDialog::saveKey()
{
    const interop::OnlineLayer* layer = chosenLayer();
    if (layer == nullptr || !context_.saveKey) {
        return;
    }
    context_.saveKey(layer->keyName, key_->text().toStdString());
    const bool removed = key_->text().isEmpty();
    key_->clear();
    setStatus(removed ? "The key for " + qs(layer->keyName) + " was removed."
                      : "The key for " + qs(layer->keyName) + " is stored in Katana's settings.");
    showDetails();
}

void OnlineDataDialog::setStatus(const QString& text, bool isError)
{
    status_->setText(text);
    status_->setStyleSheet(isError ? "color: " + theme::error().name() + ";" : QString());
}

} // namespace katana::qt
