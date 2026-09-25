#pragma once

// Small pictures of the sheets for the sheet list (docs/plotting.md, "Editing
// on the canvas"). Each is the sheet painter's own paint of the sheet at
// thumbnail size - the same function that plots it - kept until something it
// shows changes:
//
//   the sheet itself, its position and the number of sheets (its number and
//   the numbers its match lines and key plan print, overridden or not), the
//   order of the sheets,
//   the title-block values the sheets share, the project's field values, and
//   - only for a sheet that shows the drawing - the drawing's revision,
//   - only for a sheet that shows the other sheets (a drawing register, a
//     key plan, a legend of the whole set) - every sheet.
//
// So a sheet of notes is not painted again because a line was drawn, and a
// set of a hundred sheets costs one paint for the sheet that was edited. The
// editor paints the stale ones a few at a time while it is idle, the old
// picture standing in until then.

#include <cstddef>
#include <cstdint>
#include <map>
#include <string>
#include <string_view>
#include <vector>

#include <QImage>
#include <QSize>

#include "katana/cad/plotting/sheet_set.hpp"
#include "sheet_painter.hpp"

namespace katana::qt {

class SheetThumbnails {
  public:
    // Pictures fitted inside `box` logical pixels, the paper's shape kept.
    explicit SheetThumbnails(QSize box = QSize(112, 80));

    [[nodiscard]] QSize box() const { return box_; }

    // Whether sheet `index` of `set` has no picture, or one of something that
    // has changed since, for a drawing at `source`.
    [[nodiscard]] bool isStale(const katana::cad::plotting::SheetSet& set, std::size_t index,
                               const SheetSource& source) const;
    // How many sheets of `set` are stale.
    [[nodiscard]] std::size_t staleCount(const katana::cad::plotting::SheetSet& set,
                                         const SheetSource& source) const;

    // The picture of sheet `index`, painted now when it is stale, at
    // `pixelRatio` device pixels per logical pixel. Null for an index past
    // the end.
    QImage thumbnail(const katana::cad::plotting::SheetSet& set, std::size_t index,
                     const SheetSource& source, double pixelRatio = 1.0);

    // The last picture painted of the sheet with id `sheetId`, stale or not;
    // null when there is none.
    [[nodiscard]] QImage cached(std::string_view sheetId) const;

    // Forgets the pictures of sheets that are no longer in `set`.
    void prune(const katana::cad::plotting::SheetSet& set);
    void clear();

    // How many pictures have been painted, for tests of the cache.
    [[nodiscard]] int renders() const { return renders_; }

  private:
    // What a picture was painted from. The drawing's revision counts only
    // for a sheet that shows the drawing.
    struct Key {
        katana::cad::plotting::Sheet sheet;
        std::size_t index = 0;
        std::vector<std::string> order;
        katana::cad::plotting::SheetDefaults defaults;
        std::string numbering;
        std::vector<katana::cad::plotting::Revision> revisions;
        std::string fields;
        std::uint64_t revision = 0;
        double pixelRatio = 1.0;
        // The labels its match lines and key-plan outlines print: they carry
        // the number of the sheet they lead to, which that sheet's own
        // sheet_number override can change.
        std::string markLabels;
        // Every sheet, for a sheet that lists or outlines the others
        // (showsTheOtherSheets); empty for the rest.
        std::vector<katana::cad::plotting::Sheet> sheets;
    };
    struct Entry {
        Key key;
        QImage image;
    };

    [[nodiscard]] static bool matches(const Key& key, const katana::cad::plotting::SheetSet& set,
                                      std::size_t index, const SheetSource& source);

    QSize box_;
    std::map<std::string, Entry, std::less<>> entries_;
    SheetPaintCache cache_;
    int renders_ = 0;
};

// Whether any viewport of `sheet` shows the drawing (a plan, a section, a 3D
// snapshot, a legend of the layers it draws), so its picture goes stale with
// the drawing.
[[nodiscard]] bool showsTheDrawing(const katana::cad::plotting::Sheet& sheet);
// Whether any viewport of `sheet` shows what the other sheets hold: a drawing
// register (their names, scales, papers), a key plan (their plans' outlines),
// a legend of the whole set (what their plans show).
[[nodiscard]] bool showsTheOtherSheets(const katana::cad::plotting::Sheet& sheet);

} // namespace katana::qt
