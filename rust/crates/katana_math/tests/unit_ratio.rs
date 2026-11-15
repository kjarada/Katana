#[macro_use]
mod common;

mod unit_ratio {
    use katana_math::{from_metres, ratio_value, to_metres, units};

    #[test]
    fn the_definitions_are_the_statutory_ones() {
        assert_eq!(
            units::INTERNATIONAL_FOOT.numerator * 3 * 1250,
            units::INTERNATIONAL_YARD.numerator * 1250
        );
        assert_double_eq!(ratio_value(units::INTERNATIONAL_FOOT), 0.3048);
        assert_double_eq!(ratio_value(units::INTERNATIONAL_INCH), 0.0254);
        assert_double_eq!(ratio_value(units::INTERNATIONAL_YARD), 0.9144);
        assert_double_eq!(ratio_value(units::INTERNATIONAL_CHAIN), 66.0 * 0.3048);
        assert_double_eq!(ratio_value(units::INTERNATIONAL_LINK), 0.201168);
        assert_double_eq!(ratio_value(units::INTERNATIONAL_MILE), 1609.344);
        assert_double_eq!(ratio_value(units::US_SURVEY_FOOT), 12.0 / 39.37);
        assert_double_eq!(ratio_value(units::US_SURVEY_CHAIN), 66.0 * 12.0 / 39.37);
        assert_double_eq!(ratio_value(units::US_SURVEY_MILE), 5280.0 * 12.0 / 39.37);
        assert_eq!(ratio_value(units::METRE), 1.0);
    }

    #[test]
    fn a_conversion_is_exact_where_the_arithmetic_allows_it() {
        assert_eq!(to_metres(1000.0, units::INTERNATIONAL_FOOT), 304.8);
        assert_eq!(to_metres(3937.0, units::US_SURVEY_FOOT), 1200.0);
        assert_eq!(from_metres(1200.0, units::US_SURVEY_FOOT), 3937.0);
        let international = to_metres(2.0e6, units::INTERNATIONAL_FOOT);
        let survey = to_metres(2.0e6, units::US_SURVEY_FOOT);
        assert_near!(survey - international, 4.0 * 0.3048, 1e-3);
    }
}
