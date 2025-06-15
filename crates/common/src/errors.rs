use thiserror::Error;

/// Common error types for the application.
#[derive(Error, Debug, PartialEq)]
pub enum CommonError {
    /// Represents an error during I/O operations.
    #[error("I/O error: {0}")]
    IoError(String),

    /// Represents an error during data parsing or deserialization.
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Represents an invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Represents an error from an external service or API.
    #[error("External service error: {0}")]
    ExternalServiceError(String),

    /// Represents a generic, unexpected error.
    #[error("An unexpected error occurred: {0}")]
    UnexpectedError(String),

    /// Represents an item not being found.
    #[error("Item not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_display() {
        let err = CommonError::IoError("File not found".to_string());
        assert_eq!(format!("{}", err), "I/O error: File not found");
    }

    #[test]
    fn test_parse_error_display() {
        let err = CommonError::ParseError("Invalid JSON".to_string());
        assert_eq!(format!("{}", err), "Parse error: Invalid JSON");
    }

    #[test]
    fn test_invalid_configuration_display() {
        let err = CommonError::InvalidConfiguration("Missing API key".to_string());
        assert_eq!(format!("{}", err), "Invalid configuration: Missing API key");
    }

    #[test]
    fn test_external_service_error_display() {
        let err = CommonError::ExternalServiceError("Rate limit exceeded".to_string());
        assert_eq!(
            format!("{}", err),
            "External service error: Rate limit exceeded"
        );
    }

    #[test]
    fn test_unexpected_error_display() {
        let err = CommonError::UnexpectedError("Something went wrong".to_string());
        assert_eq!(
            format!("{}", err),
            "An unexpected error occurred: Something went wrong"
        );
    }

    #[test]
    fn test_not_found_error_display() {
        let err = CommonError::NotFound("User ID 123".to_string());
        assert_eq!(format!("{}", err), "Item not found: User ID 123");
    }
}
