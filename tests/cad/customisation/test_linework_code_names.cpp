// The spellings of the linework control codes are kept by the entity layer
// (include/katana/entity/linework_codes.hpp), where a customisation is read,
// and this layer still calls them by its own names: `cad::LineworkCodes` and
// `cad::validate(codes)`. What those names promise is held here - above all
// that they COMPILE the way this layer's own sources write them.

#include <gtest/gtest.h>

#include <type_traits>

#include "katana/cad/linework.hpp"
#include "katana/entity/linework_codes.hpp"

namespace katana::cad::linework_code_names_test {

// A plain `validate(codes)` from inside the cad namespace, as
// src/katana_cad/customisation/linework.cpp calls it. The codes are an entity
// type, so the call finds entity::validate through its argument as well as
// whatever this namespace calls `validate`. While cad::validate was a function
// of its own that forwarded, the two had one signature and this line did not
// compile: "call of overloaded 'validate(const LineworkCodes&)' is ambiguous".
[[nodiscard]] inline katana::core::Status plainValidate(const LineworkCodes& codes)
{
    return validate(codes);
}

} // namespace katana::cad::linework_code_names_test

TEST(LineworkCodeNames, TheCadNamesAreTheEntityTypeAndItsValidateAndNotCopiesOfThem)
{
    static_assert(std::is_same_v<katana::cad::LineworkCodes, katana::entity::LineworkCodes>);
    using Validate = katana::core::Status (*)(const katana::entity::LineworkCodes&);
    const Validate byCadName = &katana::cad::validate;
    const Validate byEntityName = &katana::entity::validate;
    EXPECT_EQ(byCadName, byEntityName) << "one function under two names";
}

TEST(LineworkCodeNames, APlainValidateInsideTheCadNamespaceIsThatSameCheck)
{
    using katana::cad::linework_code_names_test::plainValidate;
    katana::cad::LineworkCodes codes;
    EXPECT_TRUE(plainValidate(codes).ok());
    EXPECT_TRUE(katana::cad::validate(codes).ok());

    // "st" for the end, where the start is "ST": one token to the reader of a
    // field code. Both names refuse it, in the same words.
    codes.end = "st";
    const auto plain = plainValidate(codes);
    const auto qualified = katana::cad::validate(codes);
    ASSERT_FALSE(plain.ok());
    ASSERT_FALSE(qualified.ok());
    EXPECT_EQ(plain.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(plain.error().context, "start and end are \"ST\"");
    EXPECT_EQ(qualified.error().context, plain.error().context);
    EXPECT_EQ(qualified.error().message, plain.error().message);
}
