use ultrafast_gateway::error_handling::{ErrorHandler, ErrorType, OptionExt, ResultExt};
use ultrafast_gateway::gateway_error::GatewayError;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Testing Standardized Error Handling ===");

    // Test basic error types
    test_basic_error_types();

    // Test error conversion
    test_error_conversion();

    // Test validation functions
    test_validation_functions();

    // Test extension traits
    test_extension_traits();

    // Test async error handling
    test_async_error_handling().await;

    println!("✅ All error handling tests passed!");
    Ok(())
}

fn test_basic_error_types() {
    println!("Testing basic error types...");

    // Test config error
    let config_error = ErrorHandler::config_error("Invalid configuration");
    assert!(matches!(config_error, GatewayError::Config { .. }));

    // Test auth error
    let auth_error = ErrorHandler::auth_error("Invalid API key");
    assert!(matches!(auth_error, GatewayError::Auth { .. }));

    // Test rate limit error
    let rate_limit_error = ErrorHandler::rate_limit_error("Rate limit exceeded");
    assert!(matches!(rate_limit_error, GatewayError::RateLimit { .. }));

    // Test content filter error
    let content_filter_error = ErrorHandler::content_filter_error("Content blocked");
    assert!(matches!(
        content_filter_error,
        GatewayError::ContentFiltered { .. }
    ));

    // Test plugin error
    let plugin_error = ErrorHandler::plugin_error("Plugin failed");
    assert!(matches!(plugin_error, GatewayError::Plugin { .. }));

    // Test cache error
    let cache_error = ErrorHandler::cache_error("Cache unavailable");
    assert!(matches!(cache_error, GatewayError::Cache { .. }));

    // Test internal error
    let internal_error = ErrorHandler::internal_error("System failure");
    assert!(matches!(internal_error, GatewayError::Internal { .. }));

    // Test service unavailable
    let service_unavailable = ErrorHandler::service_unavailable();
    assert!(matches!(
        service_unavailable,
        GatewayError::ServiceUnavailable
    ));

    // Test invalid request
    let invalid_request = ErrorHandler::invalid_request("Bad request");
    assert!(matches!(
        invalid_request,
        GatewayError::InvalidRequest { .. }
    ));

    println!("✅ Basic error types working correctly");
}

fn test_error_conversion() {
    println!("Testing error conversion...");

    // Test log_and_convert with different error types
    let test_error = "Test error message";

    let config_error = ErrorHandler::log_and_convert(test_error, "Config test", ErrorType::Config);
    assert!(matches!(config_error, GatewayError::Config { .. }));

    let auth_error = ErrorHandler::log_and_convert(test_error, "Auth test", ErrorType::Auth);
    assert!(matches!(auth_error, GatewayError::Auth { .. }));

    let rate_limit_error =
        ErrorHandler::log_and_convert(test_error, "Rate limit test", ErrorType::RateLimit);
    assert!(matches!(rate_limit_error, GatewayError::RateLimit { .. }));

    let content_filter_error =
        ErrorHandler::log_and_convert(test_error, "Content filter test", ErrorType::ContentFilter);
    assert!(matches!(
        content_filter_error,
        GatewayError::ContentFiltered { .. }
    ));

    let plugin_error = ErrorHandler::log_and_convert(test_error, "Plugin test", ErrorType::Plugin);
    assert!(matches!(plugin_error, GatewayError::Plugin { .. }));

    let cache_error = ErrorHandler::log_and_convert(test_error, "Cache test", ErrorType::Cache);
    assert!(matches!(cache_error, GatewayError::Cache { .. }));

    let internal_error =
        ErrorHandler::log_and_convert(test_error, "Internal test", ErrorType::Internal);
    assert!(matches!(internal_error, GatewayError::Internal { .. }));

    let service_unavailable =
        ErrorHandler::log_and_convert(test_error, "Service test", ErrorType::ServiceUnavailable);
    assert!(matches!(
        service_unavailable,
        GatewayError::ServiceUnavailable
    ));

    let invalid_request =
        ErrorHandler::log_and_convert(test_error, "Request test", ErrorType::InvalidRequest);
    assert!(matches!(
        invalid_request,
        GatewayError::InvalidRequest { .. }
    ));

    println!("✅ Error conversion working correctly");
}

fn test_validation_functions() {
    println!("Testing validation functions...");

    // Test string validation
    assert!(ErrorHandler::validate_string("test", "test", 3).is_ok());
    assert!(ErrorHandler::validate_string("ab", "test", 3).is_err());

    // Test range validation
    assert!(ErrorHandler::validate_range(5, 1, 10, "test").is_ok());
    assert!(ErrorHandler::validate_range(0, 1, 10, "test").is_err());
    assert!(ErrorHandler::validate_range(11, 1, 10, "test").is_err());

    // Test require_some
    assert!(ErrorHandler::require_some(Some(42), "test").is_ok());
    assert!(ErrorHandler::require_some(None::<i32>, "test").is_err());

    // Test config validation
    let validator = |value: &i32| {
        if *value > 0 {
            Ok(())
        } else {
            Err("Value must be positive".to_string())
        }
    };

    assert!(ErrorHandler::validate_config(5, validator, "test").is_ok());
    assert!(ErrorHandler::validate_config(-1, validator, "test").is_err());

    println!("✅ Validation functions working correctly");
}

fn test_extension_traits() {
    println!("Testing extension traits...");

    // Test ResultExt with anyhow errors
    let result: Result<i32, anyhow::Error> = Ok(42);
    assert!(result.with_gateway_context("test").is_ok());

    let result: Result<i32, anyhow::Error> = Err(anyhow::anyhow!("test error"));
    assert!(result.with_gateway_context("test").is_err());

    // Test ResultExt with log_and_convert
    let result: Result<i32, String> = Ok(42);
    assert!(result.log_and_convert("test", ErrorType::Config).is_ok());

    let result: Result<i32, String> = Err("test error".to_string());
    assert!(result.log_and_convert("test", ErrorType::Config).is_err());

    // Test OptionExt
    let option: Option<i32> = Some(42);
    assert!(option.ok_or_gateway_error("test").is_ok());

    let option: Option<i32> = None;
    assert!(option.ok_or_gateway_error("test").is_err());

    println!("✅ Extension traits working correctly");
}

async fn test_async_error_handling() {
    println!("Testing async error handling...");

    // Test async operation with success
    let success_result = ErrorHandler::handle_async_operation(
        || async { Ok::<i32, anyhow::Error>(42) },
        "Async test",
        ErrorType::Config,
    )
    .await;
    assert!(success_result.is_ok());

    // Test async operation with error
    let error_result = ErrorHandler::handle_async_operation(
        || async { Err::<i32, anyhow::Error>(anyhow::anyhow!("Async error")) },
        "Async test",
        ErrorType::Config,
    )
    .await;
    assert!(error_result.is_err());

    // Test sync operation with success
    let success_result = ErrorHandler::handle_sync_operation(
        || Ok::<i32, anyhow::Error>(42),
        "Sync test",
        ErrorType::Config,
    );
    assert!(success_result.is_ok());

    // Test sync operation with error
    let error_result = ErrorHandler::handle_sync_operation(
        || Err::<i32, anyhow::Error>(anyhow::anyhow!("Sync error")),
        "Sync test",
        ErrorType::Config,
    );
    assert!(error_result.is_err());

    println!("✅ Async error handling working correctly");
}
