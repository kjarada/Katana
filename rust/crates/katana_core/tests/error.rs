mod core_result {
    use katana_core::error::{Error, ErrorCode, Result, Status};

    fn parse_positive(value: i32) -> Result<i32> {
        if value <= 0 {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "value must be positive")
                    .with_context(format!("value={value}")),
            );
        }
        Ok(value)
    }

    #[test]
    fn holds_value_on_success() {
        let result = parse_positive(7);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 7);
    }

    #[test]
    fn holds_structured_error_on_failure() {
        let result = parse_positive(-3);
        let error = result.as_ref().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            error.describe(),
            "InvalidArgument: value must be positive [value=-3]"
        );
        assert_eq!(result.unwrap_or(42), 42);
    }

    #[test]
    fn status_defaults_to_success() {
        let status: Status = Ok(());
        assert!(status.is_ok());
        let failed: Status = Err(Error::new(ErrorCode::DatabaseFailure, "disk full"));
        let Err(error) = failed else {
            panic!("the failure status must retain its error");
        };
        assert_eq!(error.describe(), "DatabaseFailure: disk full");
    }

    #[test]
    fn every_error_code_has_a_name() {
        for code in [
            ErrorCode::InvalidArgument,
            ErrorCode::InvalidState,
            ErrorCode::NotFound,
            ErrorCode::AlreadyExists,
            ErrorCode::ParseFailure,
            ErrorCode::Unsupported,
            ErrorCode::InvalidGeometry,
            ErrorCode::InvalidCrs,
            ErrorCode::InvalidSurveyObservation,
            ErrorCode::AdjustmentFailure,
            ErrorCode::TriangulationFailure,
            ErrorCode::FileImportFailure,
            ErrorCode::FileExportFailure,
            ErrorCode::DatabaseFailure,
            ErrorCode::RenderingFailure,
            ErrorCode::CommandRejected,
            ErrorCode::Internal,
        ] {
            assert_ne!(format!("{code}"), "Unknown");
        }
    }

    #[test]
    fn invalid_crs_keeps_its_wire_name_and_context() {
        let error = katana_core::Error::new(katana_core::ErrorCode::InvalidCrs, "bad CRS")
            .with_context("source");
        assert_eq!(error.describe(), "InvalidCRS: bad CRS [source]");
    }
}
