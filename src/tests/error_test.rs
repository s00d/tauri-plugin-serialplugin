#[cfg(test)]
mod tests {
    use crate::error::Error;
    use std::io;

    #[test]
    fn test_error_creation() {
        // Тест создания ошибки из строки
        let error = Error::new("Test error message");
        assert_eq!(error.to_string(), "Test error message");

        // Тест создания ошибки из IO ошибки
        let io_error = io::Error::new(io::ErrorKind::NotFound, "IO error message");
        let error = Error::from(io_error);
        assert!(error.to_string().contains("IO error message"));
    }

    #[test]
    fn test_error_conversion() {
        // Тест конвертации в IO ошибку
        let error = Error::new("Test error");
        let io_error: io::Error = error.into();
        assert!(io_error.to_string().contains("Test error"));

        // Тест конвертации из IO ошибки
        let io_error = io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied");
        let error: Error = io_error.into();
        assert!(error.to_string().contains("Permission denied"));
    }

    #[test]
    fn test_error_display() {
        let error = Error::new("Display test error");
        let display_str = format!("{}", error);
        assert_eq!(display_str, "Display test error");
    }

    #[test]
    fn test_error_debug() {
        let error = Error::new("Debug test error");
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("Debug test error"));
    }

    #[test]
    fn test_error_send_sync() {
        // Проверяем, что Error реализует Send и Sync
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    #[test]
    fn test_error_from_str() {
        let error: Error = "String error message".into();
        assert!(matches!(error, Error::String(_)));
        assert_eq!(error.to_string(), "String error message");
    }

    #[test]
    fn test_error_from_string() {
        let error = Error::new("Test error");
        assert!(matches!(error, Error::String(_)));
        assert_eq!(error.to_string(), "Test error");
    }

    #[test]
    fn test_error_from_io_error() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "Port not found");
        let error: Error = io_error.into();
        assert!(matches!(error, Error::Io(_)));
        assert!(error.to_string().contains("Port not found"));
    }

    #[test]
    fn test_error_from_string_owned() {
        let error: Error = String::from("Owned string error message").into();
        assert!(matches!(error, Error::String(_)));
        assert_eq!(error.to_string(), "Owned string error message");
    }

    #[test]
    fn test_error_clone() {
        let error = Error::new("Test error");
        let cloned = error.clone();
        assert_eq!(error.to_string(), cloned.to_string());
    }

    #[test]
    fn test_error_wrap() {
        let inner_error = Error::new("Inner error");
        let error = Error::new(format!("Wrapped error: {}", inner_error));
        assert!(matches!(error, Error::String(_)));
        assert!(error.to_string().contains("Wrapped error: Inner error"));
    }

    #[test]
    fn test_error_chain() {
        // Тест цепочки ошибок
        let io_error = io::Error::new(io::ErrorKind::NotFound, "Original error");
        let error = Error::from(io_error);
        let error = Error::new(format!("Wrapped error: {}", error));
        assert!(error.to_string().contains("Wrapped error"));
        assert!(error.to_string().contains("Original error"));
    }

    #[test]
    fn test_error_kind() {
        // Тест различных типов ошибок
        let not_found = Error::from(io::Error::new(io::ErrorKind::NotFound, "Port not found"));
        assert!(not_found.to_string().contains("Port not found"));

        let permission_denied = Error::from(io::Error::new(io::ErrorKind::PermissionDenied, "Access denied"));
        assert!(permission_denied.to_string().contains("Access denied"));

        let invalid_data = Error::from(io::Error::new(io::ErrorKind::InvalidData, "Invalid data"));
        assert!(invalid_data.to_string().contains("Invalid data"));

        let timed_out = Error::from(io::Error::new(io::ErrorKind::TimedOut, "Operation timed out"));
        assert!(timed_out.to_string().contains("Operation timed out"));
    }

    #[test]
    fn test_error_custom() {
        // Тест пользовательских ошибок
        let custom_error = Error::new("Custom error with details: port=COM1, baud=9600");
        assert!(custom_error.to_string().contains("Custom error with details"));
        assert!(custom_error.to_string().contains("port=COM1"));
        assert!(custom_error.to_string().contains("baud=9600"));
    }
} 