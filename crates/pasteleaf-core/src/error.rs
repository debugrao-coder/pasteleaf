use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        // Preserve the category without retaining OS messages, which may contain
        // private paths. Callers can then add operation-specific recovery advice.
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => "io_not_found",
            std::io::ErrorKind::PermissionDenied => "io_permission_denied",
            std::io::ErrorKind::NotADirectory => "io_not_directory",
            std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted => "io_unavailable",
            _ => "io",
        };
        Self::new(
            code,
            format!(
                "Storage I/O failed ({:?}); existing history was retained",
                e.kind()
            ),
        )
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::new(
            "database",
            format!(
                "Local database operation failed ({})",
                match e {
                    rusqlite::Error::SqliteFailure(ref e, _) => format!("{:?}", e.code),
                    _ => "invalid data".into(),
                }
            ),
        )
    }
}
impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self::new("invalid_json", "Invalid or unsupported JSON data")
    }
}
pub fn invalid(message: &str) -> Error {
    Error::new("invalid_request", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn io_categories_preserve_kind_without_os_path_details() {
        use std::io::ErrorKind;
        for (kind, code) in [
            (ErrorKind::NotFound, "io_not_found"),
            (ErrorKind::PermissionDenied, "io_permission_denied"),
            (ErrorKind::NotADirectory, "io_not_directory"),
            (ErrorKind::WouldBlock, "io_unavailable"),
            (ErrorKind::TimedOut, "io_unavailable"),
            (ErrorKind::Interrupted, "io_unavailable"),
            (ErrorKind::Other, "io"),
        ] {
            let error = Error::from(std::io::Error::new(kind, "/synthetic/private/path"));
            assert_eq!(error.code, code);
            assert!(!error.message.contains("/synthetic/private/path"));
            assert!(error.message.contains("existing history was retained"));
        }
    }
}
