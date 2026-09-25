// Document's annotation members (include/katana/cad/document.hpp), kept here
// with the rest of the annotation code, as the sheet set's are kept with the
// sheets (plotting/sheet_store.cpp).

#include <cmath>
#include <functional>
#include <memory>
#include <string>
#include <utility>

#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/annotation.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// The metadata key the scale is kept under. The storage layer keeps a key it
// does not read and writes it back unchanged (ProjectMetadata::unknownKeys),
// so the scale travels with the project with no schema change.
constexpr std::string_view kAnnotationScaleKey = "annotation_scale";

// One change of the scale: the key's state before and after, applied by the
// document - the SheetSetCommand shape (plotting/sheet_store.cpp).
class AnnotationScaleCommand final : public katana::commands::Command {
  public:
    using Apply = std::function<void(bool after)>;
    explicit AnnotationScaleCommand(Apply apply) : apply_(std::move(apply)) {}

    [[nodiscard]] std::string_view name() const override { return "SET_ANNOTATION_SCALE"; }
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
    Apply apply_;
};

} // namespace

double Document::annotationScale() const
{
    const auto found = metadata_.unknownKeys.find(std::string(kAnnotationScaleKey));
    if (found != metadata_.unknownKeys.end()) {
        // A value that is not a positive number - edited by hand, or damaged
        // - is not a scale; the default is used and the key left as it is.
        if (const auto scale = katana::core::parseFiniteDouble(found->second);
            scale && *scale > 0.0) {
            return *scale;
        }
    }
    return katana::entity::kDefaultAnnotationScale;
}

Status Document::setAnnotationScale(double scale)
{
    if (!std::isfinite(scale) || !(scale > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "an annotation scale is a positive number, the N of 1 : N",
                         std::to_string(scale));
    }
    const std::string key(kAnnotationScaleKey);
    const auto found = metadata_.unknownKeys.find(key);
    std::optional<std::string> before;
    if (found != metadata_.unknownKeys.end()) {
        before = found->second;
    }
    std::optional<std::string> after = katana::core::formatExactReal(scale);
    // The default is stored as no key at all, so choosing it again after a
    // change undoes to a project exactly as it was.
    if (scale == katana::entity::kDefaultAnnotationScale) {
        after.reset();
    }
    if (before == after) {
        return {}; // nothing changed, and nothing to undo
    }
    auto apply = [this, key, before, after](bool toAfter) {
        const std::optional<std::string>& state = toAfter ? after : before;
        if (state) {
            metadata_.unknownKeys.insert_or_assign(key, *state);
        } else {
            metadata_.unknownKeys.erase(key);
        }
    };
    return execute(std::make_unique<AnnotationScaleCommand>(std::move(apply)));
}

} // namespace katana::cad
