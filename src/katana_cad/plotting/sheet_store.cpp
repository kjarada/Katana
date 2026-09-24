// Document's sheet-set members (include/katana/cad/document.hpp), kept here
// with the rest of the sheet code rather than in document.cpp: the document
// only carries the JSON; everything that knows what a sheet is lives in
// plotting/.

#include <optional>
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

// One sheet-set change: the stored JSON before and after, applied by the
// document. nullopt is "no sheets key", so undoing the first sheet ever added
// leaves the metadata exactly as it was.
class SheetSetCommand final : public katana::commands::Command {
  public:
    using Apply = std::function<void(const std::optional<std::string>&)>;

    SheetSetCommand(std::string name, std::optional<std::string> before,
                    std::optional<std::string> after, Apply apply)
        : name_(std::move(name)), before_(std::move(before)), after_(std::move(after)),
          apply_(std::move(apply))
    {
    }

    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const katana::commands::CommandContext&) const override
    {
        return {};
    }
    [[nodiscard]] Status execute(katana::commands::CommandContext&) override
    {
        apply_(after_);
        return {};
    }
    [[nodiscard]] Status undo(katana::commands::CommandContext&) override
    {
        apply_(before_);
        return {};
    }
    [[nodiscard]] Status redo(katana::commands::CommandContext&) override
    {
        apply_(after_);
        return {};
    }

  private:
    std::string name_;
    std::optional<std::string> before_;
    std::optional<std::string> after_;
    Apply apply_;
};

} // namespace

struct Document::SheetCache {
    // What the set was parsed from: whether the key was there, and its text.
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
    if (const Status status = sheetSetStatus();
        !status && status.error().code == ErrorCode::Unsupported) {
        return makeError(ErrorCode::CommandRejected,
                         "the sheets were written by a newer version of Katana; they are kept as "
                         "they are",
                         status.error().describe());
    }
    std::optional<std::string> after;
    if (!(sheets == plotting::SheetSet{})) {
        auto json = plotting::sheetSetToJson(sheets);
        if (!json) {
            return json.error();
        }
        after = std::move(*json);
    }
    std::optional<std::string> before;
    if (const auto found = metadata_.unknownKeys.find(std::string(kSheetsKey));
        found != metadata_.unknownKeys.end()) {
        before = found->second;
    }
    if (before == after) {
        return {}; // nothing changed, and nothing to undo
    }
    auto apply = [this](const std::optional<std::string>& text) {
        if (text) {
            metadata_.unknownKeys.insert_or_assign(std::string(kSheetsKey), *text);
        } else {
            metadata_.unknownKeys.erase(std::string(kSheetsKey));
        }
    };
    return execute(std::make_unique<SheetSetCommand>(std::move(stepName), std::move(before),
                                                     std::move(after), std::move(apply)));
}

} // namespace katana::cad
