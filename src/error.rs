use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    General,
    TypeMismatch,
    InvalidSyntax,
    OutOfRange,
    Fetch,
    Yaml,
    Http,
    UnsupportedSetting,
    TeamNotFound,
}

impl ErrorKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ErrorKind::General => "Error",
            ErrorKind::TypeMismatch => "TypeError",
            ErrorKind::InvalidSyntax => "SyntaxError",
            ErrorKind::OutOfRange => "RangeError",
            ErrorKind::Fetch => "FetchError",
            ErrorKind::Yaml => "YAMLException",
            ErrorKind::Http => "HttpError",
            ErrorKind::UnsupportedSetting => "UnSupportedSettingError",
            ErrorKind::TeamNotFound => "TeamNotFoundError",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    status: Option<u16>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error {
            kind,
            message: message.into(),
            status: None,
        }
    }

    pub fn general(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::General, message)
    }

    pub fn type_mismatch(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::TypeMismatch, message)
    }

    #[must_use]
    pub fn missing_property(property: &str) -> Self {
        Self::type_mismatch(format!(
            "Cannot read properties of undefined (reading '{property}')"
        ))
    }

    pub fn unsupported_setting(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::UnsupportedSetting, message)
    }

    pub fn team_not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::TeamNotFound, message)
    }

    pub fn http(status: u16, message: impl Into<String>) -> Self {
        Error {
            kind: ErrorKind::Http,
            message: message.into(),
            status: Some(status),
        }
    }

    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn status(&self) -> Option<u16> {
        self.status
    }

    #[must_use]
    pub fn has_status(&self, status: u16) -> bool {
        self.status == Some(status)
    }

    #[must_use]
    pub fn with_message(self, message: String) -> Self {
        Error { message, ..self }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(formatter, "{}", self.kind)
        } else {
            write!(formatter, "{}: {}", self.kind, self.message)
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_prefixes_the_message_with_the_kind_name() {
        assert_eq!(
            Error::missing_property("login").to_string(),
            "TypeError: Cannot read properties of undefined (reading 'login')"
        );
        assert_eq!(Error::general("").to_string(), "Error");
        assert_eq!(
            Error::unsupported_setting("x").to_string(),
            "UnSupportedSettingError: x"
        );
    }

    #[test]
    fn http_errors_expose_their_status_and_keep_it_when_the_message_changes() {
        let error = Error::http(404, "Not Found").with_message("Not found. Reason".to_string());
        assert!(error.has_status(404));
        assert_eq!(error.status(), Some(404));
        assert_eq!(error.kind(), ErrorKind::Http);
        assert_eq!(error.message(), "Not found. Reason");
        assert_eq!(Error::general("x").status(), None);
    }
}
