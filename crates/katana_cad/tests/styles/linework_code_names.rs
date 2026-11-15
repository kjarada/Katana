// The spellings of the linework control codes are kept by the entity layer
// (entity's linework_codes.rs), where a customisation is read, and this layer
// still calls them by its own names: `linework::LineworkCodes` and
// `linework::validate(codes)` (tests/cad/customisation/test_linework_code_names.cpp).
// What those names promise is held here: that they are the entity's type and
// its validate, re-exported, and not copies of them.

use katana_cad::linework;
use katana_core::{ErrorCode, Status};

// A plain `validate(codes)` from inside a module that takes the cad name, as
// linework.rs calls it. The C++ case is about overload ambiguity, which Rust
// has no counterpart of: two functions under one name in one scope do not
// compile at all. What stands is that the cad name is the entity's function.
fn plain_validate(codes: &linework::LineworkCodes) -> Status {
    use katana_cad::linework::validate;
    validate(codes)
}

// Compiles only when the cad name is the entity's type: the C++
// static_assert(std::is_same_v<...>).
fn same_type(codes: katana_cad::linework::LineworkCodes) -> katana_entity::LineworkCodes {
    codes
}

mod linework_code_names {
    use super::*;

    #[test]
    fn the_cad_names_are_the_entity_type_and_its_validate_and_not_copies_of_them() {
        let codes = same_type(linework::LineworkCodes::default());
        assert_eq!(codes, katana_entity::LineworkCodes::default());
        type Validate = fn(&katana_entity::LineworkCodes) -> Status;
        let by_cad_name: Validate = linework::validate::<katana_entity::LineworkCodes>;
        let by_entity_name: Validate = katana_entity::validate::<katana_entity::LineworkCodes>;
        assert!(
            std::ptr::fn_addr_eq(by_cad_name, by_entity_name),
            "one function under two names"
        );
    }

    #[test]
    fn a_plain_validate_inside_the_cad_namespace_is_that_same_check() {
        let mut codes = linework::LineworkCodes::default();
        assert!(plain_validate(&codes).is_ok());
        assert!(linework::validate(&codes).is_ok());

        // "st" for the end, where the start is "ST": one token to the reader of
        // a field code. Both names refuse it, in the same words.
        codes.end = String::from("st");
        let plain = plain_validate(&codes).expect_err("refused");
        let qualified = linework::validate(&codes).expect_err("refused");
        assert_eq!(plain.code, ErrorCode::InvalidArgument);
        assert_eq!(plain.context, "start and end are \"ST\"");
        assert_eq!(qualified.context, plain.context);
        assert_eq!(qualified.message, plain.message);
    }
}
