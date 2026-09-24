#include "survey/reduction_report_view.hpp"

#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QSplitter>
#include <QTextBrowser>
#include <QVBoxLayout>

#include <format>
#include <string_view>
#include <utility>

#include "theme.hpp"

namespace katana::qt {

namespace survey = katana::survey;

namespace {

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

// The entities survey::renderHtml escapes, back to text for the list.
std::string unescaped(std::string_view text)
{
    std::string out;
    out.reserve(text.size());
    static constexpr std::pair<std::string_view, char> kEntities[] = {
        {"&amp;", '&'}, {"&lt;", '<'}, {"&gt;", '>'}, {"&quot;", '"'}, {"&#39;", '\''}};
    for (std::size_t i = 0; i < text.size();) {
        bool matched = false;
        if (text[i] == '&') {
            for (const auto& [entity, c] : kEntities) {
                if (text.substr(i, entity.size()) == entity) {
                    out += c;
                    i += entity.size();
                    matched = true;
                    break;
                }
            }
        }
        if (!matched) {
            out += text[i++];
        }
    }
    return out;
}

std::size_t count(std::string_view text, std::string_view what)
{
    std::size_t n = 0;
    for (std::size_t at = text.find(what); at != std::string_view::npos;
         at = text.find(what, at + what.size())) {
        ++n;
    }
    return n;
}

// `a` over `b` at `alpha`: the tint behind a marked cell, made from the theme
// rather than written as a colour of its own.
QColor blended(const QColor& a, const QColor& b, double alpha)
{
    return QColor::fromRgbF(static_cast<float>(a.redF() * alpha + b.redF() * (1.0 - alpha)),
                            static_cast<float>(a.greenF() * alpha + b.greenF() * (1.0 - alpha)),
                            static_cast<float>(a.blueF() * alpha + b.blueF() * (1.0 - alpha)));
}

QString screenStyleSheet()
{
    const QColor background = theme::panel();
    return QString("body { color: %1; background-color: %2; }\n"
                   "h1 { font-size: 12pt; color: %1; }\n"
                   "h2 { font-size: 11pt; color: %3; margin-top: 14px; }\n"
                   "table { border-color: %4; }\n"
                   "th { background-color: %5; color: %1; font-weight: 600; }\n"
                   "td.alarm { color: %6; font-weight: bold; background-color: %7; }\n"
                   "p.empty { color: %8; }\n")
        .arg(theme::text().name(), background.name(), theme::accent().name(),
             theme::border().name(), theme::raised().name(), theme::error().name(),
             blended(theme::error(), background, 0.22).name(), theme::textMuted().name());
}

void appendClipped(std::string& target, std::string_view what, std::size_t shown,
                   std::size_t total)
{
    if (total <= shown) {
        return;
    }
    target += target.empty() ? "" : ", ";
    target += std::format("{} ({} of {})", what, shown, total);
}

} // namespace

std::string screenHtml(const std::string& html, std::vector<ReportSection>* sections)
{
    std::string_view in = html;
    std::string out;
    out.reserve(html.size() + 4096);
    // The report's own style sheet is for paper; the view sets its own.
    if (const auto style = in.find("<style>"); style != std::string_view::npos) {
        if (const auto end = in.find("</style>", style); end != std::string_view::npos) {
            out.append(in.substr(0, style));
            in.remove_prefix(end + std::string_view("</style>").size());
        }
    }
    std::vector<std::size_t> headingAt; // in `out`, for counting marks per section
    std::size_t number = 0;
    constexpr std::string_view kHeading = "<h2>";
    while (!in.empty()) {
        const std::size_t table = in.find("<table");
        const std::size_t heading = in.find(kHeading);
        const std::size_t next = std::min(table, heading);
        if (next == std::string_view::npos) {
            out.append(in);
            break;
        }
        out.append(in.substr(0, next));
        in.remove_prefix(next);
        if (next == heading) {
            const std::string anchor = std::format("section-{}", ++number);
            const std::size_t close = in.find("</h2>");
            const std::string_view title =
                close == std::string_view::npos ? std::string_view{}
                                                : in.substr(kHeading.size(), close - kHeading.size());
            if (sections != nullptr) {
                sections->push_back({unescaped(title), anchor, 0});
            }
            headingAt.push_back(out.size());
            out += "<a name=\"" + anchor + "\"></a>";
            out.append(kHeading);
            in.remove_prefix(kHeading.size());
        } else {
            // Borders QTextDocument draws: it takes them from the table's
            // attributes, not from a CSS border on the cells.
            out += "<table border=\"1\" cellspacing=\"0\" cellpadding=\"3\"";
            in.remove_prefix(std::string_view("<table").size());
        }
    }
    if (sections != nullptr) {
        for (std::size_t i = 0; i < headingAt.size() && i < sections->size(); ++i) {
            const std::size_t end = i + 1 < headingAt.size() ? headingAt[i + 1] : out.size();
            (*sections)[i].marked = count(std::string_view(out).substr(headingAt[i],
                                                                       end - headingAt[i]),
                                          "class=\"alarm");
        }
    }
    return out;
}

survey::ReductionReport displayReport(const survey::ReductionReport& report, std::size_t maxRows)
{
    survey::ReductionReport shown;
    shown.createdUtc = report.createdUtc;
    shown.input = report.input;
    shown.settings = report.settings;
    shown.misclosures = report.misclosures;
    std::string clipped;
    const auto take = [maxRows, &clipped](const auto& from, auto& to, std::string_view what) {
        const std::size_t n = std::min(from.size(), maxRows);
        to.assign(from.begin(), from.begin() + static_cast<std::ptrdiff_t>(n));
        appendClipped(clipped, what, n, from.size());
    };
    take(report.input.warnings, shown.input.warnings, "file warnings");
    take(report.setups, shown.setups, "setups");
    take(report.observations, shown.observations, "observations");
    take(report.facePairs, shown.facePairs, "face pairs");
    for (const survey::AdjustmentReport& adjustment : report.adjustments) {
        survey::AdjustmentReport cut = adjustment;
        take(adjustment.residuals, cut.residuals, "residuals");
        take(adjustment.ellipses, cut.ellipses, "error ellipses");
        shown.adjustments.push_back(std::move(cut));
    }
    take(report.coordinates, shown.coordinates, "coordinates");
    take(report.warnings, shown.warnings, "warnings");
    if (!clipped.empty()) {
        shown.warnings.insert(shown.warnings.begin(),
                              {"This view shows the first rows only: " + clipped +
                                   ". The whole report is kept with the survey job; export it "
                                   "(Survey > Survey Jobs, Export Report) to read every row.",
                               {}});
    }
    return shown;
}

ReductionReportView::ReductionReportView(const QString& name, QWidget* parent) : QWidget(parent)
{
    setObjectName(name);
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    summary_ = new QLabel(this);
    summary_->setObjectName(name + "Summary");
    summary_->setWordWrap(true);
    layout->addWidget(summary_);
    auto* splitter = new QSplitter(Qt::Horizontal, this);
    list_ = new QListWidget(splitter);
    list_->setObjectName(name + "Sections");
    list_->setToolTip("The report's sections: choose one to go to it");
    browser_ = new QTextBrowser(splitter);
    browser_->setObjectName(name + "Browser");
    browser_->setOpenLinks(false);
    browser_->document()->setDefaultStyleSheet(screenStyleSheet());
    splitter->addWidget(list_);
    splitter->addWidget(browser_);
    splitter->setStretchFactor(0, 0);
    splitter->setStretchFactor(1, 1);
    list_->setMinimumWidth(90);
    splitter->setSizes({140, 600});
    layout->addWidget(splitter, 1);
    connect(list_, &QListWidget::currentRowChanged, this, [this](int row) { showSection(row); });
    showNote("No report yet.");
}

void ReductionReportView::setReportHtml(const std::string& html)
{
    sections_.clear();
    const std::string screen = screenHtml(html, &sections_);
    // The default style sheet applies to what is set after it.
    browser_->document()->setDefaultStyleSheet(screenStyleSheet());
    browser_->setHtml(qs(screen));
    list_->blockSignals(true);
    list_->clear();
    for (const ReportSection& section : sections_) {
        auto* item = new QListWidgetItem(
            section.marked == 0 ? qs(section.title)
                                : QString("%1  [%2 marked]").arg(qs(section.title)).arg(section.marked),
            list_);
        if (section.marked > 0) {
            item->setForeground(theme::error());
            QFont bold = item->font();
            bold.setBold(true);
            item->setFont(bold);
            item->setToolTip("Holds a flagged or rejected outlier, a failed test or a value "
                             "outside its tolerance");
        }
    }
    list_->blockSignals(false);
    const std::size_t total = marked();
    summary_->setText(total == 0 ? QString("%1 sections; nothing is flagged, rejected or outside "
                                           "a tolerance.")
                                       .arg(sections_.size())
                                 : QString("%1 sections; %2 value(s) flagged, rejected, failed or "
                                           "outside a tolerance - marked in red.")
                                       .arg(sections_.size())
                                       .arg(total));
    summary_->setStyleSheet(QString("color: %1").arg(
        total == 0 ? theme::textMuted().name() : theme::error().name()));
}

void ReductionReportView::showNote(const QString& note)
{
    sections_.clear();
    list_->clear();
    browser_->setPlainText(note);
    summary_->setText(QString());
}

std::size_t ReductionReportView::marked() const
{
    std::size_t total = 0;
    for (const ReportSection& section : sections_) {
        total += section.marked;
    }
    return total;
}

void ReductionReportView::showSection(int index)
{
    if (index < 0 || static_cast<std::size_t>(index) >= sections_.size()) {
        return;
    }
    browser_->scrollToAnchor(qs(sections_[static_cast<std::size_t>(index)].anchor));
}

} // namespace katana::qt
