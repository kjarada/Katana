// Document's sheet-set members (include/katana/cad/document.hpp), kept here
// with the rest of the sheet code rather than in document.cpp: the document
// only carries the JSON; everything that knows what a sheet is lives in
// plotting/.

#include <functional>
#include <memory>
#include <string>
#include <utility>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/sheet_set.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// The metadata key the sheet set is stored under. The storage layer keeps a
// key it does not read and writes it back unchanged (ProjectMetadata::
// unknownKeys), which is what lets sheets live in a project without a schema
// migration.
constexpr std::string_view kSheetsKey = "sheets";

// One sheet-set change: the document's state before and after, applied by
// the document. Each side is the stored JSON together with the set parsed
// from it (Document::SheetCache), so an undo or redo puts the text back in
// the metadata and the parsed set back in the cache without parsing - the
// parse was most of what an edit of a large set cost (bench_sheets.cpp).
class SheetSetCommand final : public katana::commands::Command {
  public:
    // Called with true to apply the state after the change, false before.
    using Apply = std::function<void(bool after)>;

    SheetSetCommand(std::string name, Apply apply)
        : name_(std::move(name)), apply_(std::move(apply))
    {
    }

    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const katana::commands::CommandContext&) const override
    {
        return {};
    }
    [[nodiscard]] Status execute(katana::commands::CommandContext&) override
    {
        apply_(true);
        return {};
    }
    [[nodiscard]] Status undo(katana::commands::CommandContext&) override
    {
        apply_(false);
        return {};
    }
    [[nodiscard]] Status redo(katana::commands::CommandContext&) override
    {
        apply_(true);
        return {};
    }

  private:
    std::string name_;
    Apply apply_;
};

} // namespace

struct Document::SheetCache {
    // What the set was parsed from: whether the key was there, and its text.
    // A state with no key (present false) is one too, so undoing the first
    // sheet ever added leaves the metadata exactly as it was.
    bool present = false;
    std::string text;
    plotting::SheetSet set;
    Status status;
};

const plotting::SheetSet& Document::sheetSet() const
{
    const auto found = metadata_.unknownKeys.find(std::string(kSheetsKey));
    const bool present = found != metadata_.unknownKeys.end();
    // Compared by content, not by a revision counter, because setMetadata
    // replaces the whole metadata - sheets key included - and must be seen.
    if (sheetCache_ && sheetCache_->present == present &&
        (!present || sheetCache_->text == found->second)) {
        return sheetCache_->set;
    }
    auto cache = std::make_shared<SheetCache>();
    cache->present = present;
    if (present) {
        cache->text = found->second;
        auto parsed = plotting::sheetSetFromJson(cache->text);
        if (parsed) {
            cache->set = std::move(*parsed);
        } else {
            cache->status = parsed.error();
        }
    }
    sheetCache_ = std::move(cache);
    return sheetCache_->set;
}

Status Document::sheetSetStatus() const
{
    (void)sheetSet();
    return sheetCache_->status;
}

Status Document::setSheetSet(const plotting::SheetSet& sheets, std::string stepName)
{
    // Brings the cache up to date with the metadata: it is the step's "before".
    if (const Status status = sheetSetStatus();
        !status && status.error().code == ErrorCode::Unsupported) {
        return makeError(ErrorCode::CommandRejected,
                         "the sheets were written by a newer version of Katana; they are kept as "
                         "they are",
                         status.error().describe());
    }
    auto after = std::make_shared<SheetCache>();
    // An empty set is stored as no key at all.
    if (!(sheets == plotting::SheetSet{})) {
        auto json = plotting::sheetSetToJson(sheets);
        if (!json) {
            return json.error();
        }
        after->present = true;
        after->text = std::move(*json);
        after->set = sheets;
    }
    std::shared_ptr<const SheetCache> before = sheetCache_;
    if (before->present == after->present && before->text == after->text) {
        return {}; // nothing changed, and nothing to undo
    }
    auto apply = [this, before = std::move(before),
                  after = std::shared_ptr<const SheetCache>(std::move(after))](bool toAfter) {
        const std::shared_ptr<const SheetCache>& state = toAfter ? after : before;
        if (state->present) {
            metadata_.unknownKeys.insert_or_assign(std::string(kSheetsKey), state->text);
        } else {
            metadata_.unknownKeys.erase(std::string(kSheetsKey));
        }
        sheetCache_ = state;
    };
    return execute(std::make_unique<SheetSetCommand>(std::move(stepName), std::move(apply)));
}

} // namespace katana::cad
