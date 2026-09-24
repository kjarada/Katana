#pragma once

// A reduction report on screen: the HTML survey::renderHtml writes, in a
// QTextBrowser, with a list of its sections beside it to jump to, and what
// went wrong - flagged or rejected outliers, a failed global test, a face
// pair or misclosure outside its tolerance - marked so it is seen first.
//
// The HTML is the report's own (the one a job keeps and exports); only its
// LOOK is changed here, for the application's dark theme: the report's
// print style sheet is taken out and one made from the theme's colours is
// used instead, so the saved or printed report stays black on white while the
// one on screen is readable beside the drawing. Anchors are added before each
// section heading for the list to jump to. Nothing in the report's content
// is changed.
//
// Object names: <name> (the view), <name>Sections (the list),
// <name>Browser (the text), <name>Summary (what is marked, in a line).

#include <QWidget>

#include <cstddef>
#include <string>
#include <vector>

#include "katana/survey/reduction_report.hpp"

class QLabel;
class QListWidget;
class QResizeEvent;
class QSplitter;
class QTextBrowser;

namespace katana::qt {

// One section of a report as the view lists it.
struct ReportSection {
    std::string title;
    std::string anchor;
    std::size_t marked = 0; // cells marked as alarms in it
};

// The report's HTML made ready for the screen: its style sheet removed,
// every <table> given explicit borders (QTextDocument draws no cell border
// from CSS alone), and an anchor before every section heading. `sections`
// receives the sections in order. Exposed for the tests.
[[nodiscard]] std::string screenHtml(const std::string& html, std::vector<ReportSection>* sections);

// A copy of `report` with each long table cut to `maxRows` rows and a warning
// saying so, for the screen: a job of a million observations has a report of
// hundreds of megabytes, which no text view lays out in reasonable time. The
// job keeps, and Export writes, the whole report.
[[nodiscard]] katana::survey::ReductionReport displayReport(const katana::survey::ReductionReport& report,
                                                           std::size_t maxRows);

class ReductionReportView final : public QWidget {
  public:
    ReductionReportView(const QString& name, QWidget* parent);

    // Shows a report rendered by survey::renderHtml.
    void setReportHtml(const std::string& html);
    // Shows a sentence instead of a report ("No preview yet").
    void showNote(const QString& note);

    [[nodiscard]] const std::vector<ReportSection>& sections() const { return sections_; }
    [[nodiscard]] std::size_t marked() const;
    // Scrolls to the section at `index` of sections().
    void showSection(int index);

    // True when the sections are listed above the text rather than beside it.
    [[nodiscard]] bool sectionsAbove() const;

  protected:
    // The sections go beside the text when the view is wide and above it
    // when it is narrow - two reports side by side (Survey Jobs' preview)
    // would otherwise give half of each to a list of headings.
    void resizeEvent(QResizeEvent* event) override;

  private:
    QSplitter* splitter_ = nullptr;
    QListWidget* list_ = nullptr;
    QTextBrowser* browser_ = nullptr;
    QLabel* summary_ = nullptr;
    std::vector<ReportSection> sections_;
};

} // namespace katana::qt
