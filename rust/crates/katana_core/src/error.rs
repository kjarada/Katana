use std::fmt;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum ErrorCode {
    InvalidArgument,
    InvalidState,
    NotFound,
    AlreadyExists,
    ParseFailure,
    Unsupported,
    InvalidGeometry,
    InvalidCrs,
    InvalidSurveyObservation,
    AdjustmentFailure,
    TriangulationFailure,
    FileImportFailure,
    FileExportFailure,
    DatabaseFailure,
    RenderingFailure,
    CommandRejected,
    #[default]
    Internal,
}

pub const fn to_string(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidArgument => "InvalidArgument",
        ErrorCode::InvalidState => "InvalidState",
        ErrorCode::NotFound => "NotFound",
        ErrorCode::AlreadyExists => "AlreadyExists",
        ErrorCode::ParseFailure => "ParseFailure",
        ErrorCode::Unsupported => "Unsupported",
        ErrorCode::InvalidGeometry => "InvalidGeometry",
        ErrorCode::InvalidCrs => "InvalidCRS",
        ErrorCode::InvalidSurveyObservation => "InvalidSurveyObservation",
        ErrorCode::AdjustmentFailure => "AdjustmentFailure",
        ErrorCode::TriangulationFailure => "TriangulationFailure",
        ErrorCode::FileImportFailure => "FileImportFailure",
        ErrorCode::FileExportFailure => "FileExportFailure",
        ErrorCode::DatabaseFailure => "DatabaseFailure",
        ErrorCode::RenderingFailure => "RenderingFailure",
        ErrorCode::CommandRejected => "CommandRejected",
        ErrorCode::Internal => "Internal",
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(to_string(*self))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub context: String,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: String::new(),
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = context.into();
        self
    }

    pub fn describe(&self) -> String {
        if self.context.is_empty() {
            format!("{}: {}", self.code, self.message)
        } else {
            format!("{}: {} [{}]", self.code, self.message, self.context)
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.describe())
    }
}

impl std::error::Error for Error {}

pub fn make_error(
    code: ErrorCode,
    message: impl Into<String>,
    context: impl Into<String>,
) -> Error {
    Error::new(code, message).with_context(context)
}

pub type Result<T> = std::result::Result<T, Error>;
pub type Status = Result<()>;
