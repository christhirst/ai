use ai::config::{AppConfig, DatabaseConfig, GrpcConfig};
use ai::db::{
    AppDb, execute_surrealql, get_defined_tables, get_table_schema_summary, init_memory_db,
    insert_dynamic_records, set_table_comment,
};
use ai::grpc::{
    ExecuteSurrealQlRequest, FactCheckEntriesRequest, GetTableInfoRequest, ListTablesRequest,
    PopulateTableIntervalRequest, PopulateTableRequest, TablePopulatorService,
    TablePopulatorServiceImpl, TablePopulatorServiceServer, connect_client, parse_json_response,
    sanitize_and_map_records,
};
use serde_json::json;
use std::sync::Arc;
use tonic::Request;
use tonic::transport::Server;

#[test]
fn test_parse_json_response_variations() {
    // 1. Standard JSON Array
    let raw_array = r#"[{"title": "Book 1", "year": 2020}, {"title": "Book 2", "year": 2021}]"#;
    let res = parse_json_response(raw_array).expect("Failed to parse standard array");
    assert_eq!(res.len(), 2);
    assert_eq!(res[0]["title"], "Book 1");

    // 2. Markdown Wrapped ```json ... ```
    let markdown_wrapped = r#"```json
    [
        {"city": "Berlin", "cases": 12},
        {"city": "Hamburg", "cases": 8}
    ]
    ```"#;
    let res = parse_json_response(markdown_wrapped).expect("Failed to parse markdown wrapped");
    assert_eq!(res.len(), 2);
    assert_eq!(res[0]["city"], "Berlin");

    // 3. Nested "records" object
    let nested_records = r#"{
        "status": "success",
        "records": [
            {"name": "Alice", "score": 95},
            {"name": "Bob", "score": 88}
        ]
    }"#;
    let res = parse_json_response(nested_records).expect("Failed to parse nested records");
    assert_eq!(res.len(), 2);
    assert_eq!(res[0]["name"], "Alice");

    // 4. Single JSON Object
    let single_obj = r#"{"incident": "Robbery", "date": "2026-01-01"}"#;
    let res = parse_json_response(single_obj).expect("Failed to parse single object");
    assert_eq!(res.len(), 1);
    assert_eq!(res[0]["incident"], "Robbery");

    // 5. Embedded in text
    let embedded_text = r#"Here is the extracted data you requested:
    [
        {"metric": "GDP", "value": 4.5}
    ]
    I hope this helps!"#;
    let res = parse_json_response(embedded_text).expect("Failed to parse embedded JSON");
    assert_eq!(res.len(), 1);
    assert_eq!(res[0]["metric"], "GDP");
}

#[tokio::test]
async fn test_table_ddl_and_prompt_comment_storage() {
    let db = init_memory_db("test_ns", "test_db")
        .await
        .expect("Failed to init in-memory database");
    let app_db = AppDb::Local(db);

    // 1. Define table schema with fields
    let ddl = "DEFINE TABLE crime_stats SCHEMAFULL; \
               DEFINE FIELD city ON TABLE crime_stats TYPE string; \
               DEFINE FIELD incident_count ON TABLE crime_stats TYPE int;";
    execute_surrealql(&app_db, ddl)
        .await
        .expect("Failed to execute DDL");

    // 2. Set the prompt as the table COMMENT
    let prompt = "Extract total reported crime incidents for Berlin and Munich in 2025";
    set_table_comment(&app_db, "crime_stats", prompt)
        .await
        .expect("Failed to set table comment");

    // 3. Verify schema summary and comment provenance
    let summary = get_table_schema_summary(&app_db, "crime_stats")
        .await
        .expect("Failed to get table schema summary");
    assert_eq!(summary.table_name, "crime_stats");
    assert_eq!(summary.comment, prompt);
    assert!(summary.fields.iter().any(|f| f.contains("city")));
    assert!(summary.fields.iter().any(|f| f.contains("incident_count")));

    // 4. Insert dynamic JSON records
    let records = vec![
        json!({
            "city": "Berlin",
            "incident_count": 1420
        }),
        json!({
            "city": "Munich",
            "incident_count": 680
        }),
    ];
    let inserted = insert_dynamic_records(&app_db, "crime_stats", &records)
        .await
        .expect("Failed to insert dynamic records");
    assert_eq!(inserted.len(), 2);

    // 5. Verify defined tables
    let tables = get_defined_tables(&app_db).await.unwrap();
    assert!(tables.contains(&"crime_stats".to_string()));
}

#[tokio::test]
async fn test_grpc_service_methods() {
    let db = init_memory_db("test_ns", "test_db")
        .await
        .expect("Failed to init db");
    let app_db = Arc::new(AppDb::Local(db));

    let config = Arc::new(AppConfig {
        gemini_api_key: "test_key".to_string(),
        model: "gemini-3.7-flash".to_string(),
        db: DatabaseConfig {
            endpoint: "mem://".to_string(),
            namespace: "test_ns".to_string(),
            database: "test_db".to_string(),
            username: None,
            password: None,
        },
        grpc: GrpcConfig {
            host: "127.0.0.1".to_string(),
            port: 50051,
            ..Default::default()
        },
        ..Default::default()
    });

    let service = TablePopulatorServiceImpl::new(config, app_db.clone());

    // 1. Execute SurrealQL via RPC
    let ddl_resp = service
        .execute_surreal_ql(Request::new(ExecuteSurrealQlRequest {
            sql: "DEFINE TABLE inventory SCHEMAFULL; DEFINE FIELD item ON TABLE inventory TYPE string; DEFINE FIELD qty ON TABLE inventory TYPE int;".to_string(),
            namespace: None,
            database: None,
        }))
        .await
        .expect("RPC execute_surreal_ql failed")
        .into_inner();
    assert!(ddl_resp.success);

    // 2. Set comment on the table
    set_table_comment(
        &*app_db,
        "inventory",
        "Track inventory levels for warehouse A",
    )
    .await
    .unwrap();

    // 3. Get table info via RPC
    let info_resp = service
        .get_table_info(Request::new(GetTableInfoRequest {
            table_name: "inventory".to_string(),
            namespace: None,
            database: None,
        }))
        .await
        .expect("RPC get_table_info failed")
        .into_inner();
    assert_eq!(info_resp.table_name, "inventory");
    assert_eq!(info_resp.comment, "Track inventory levels for warehouse A");
    assert!(info_resp.fields.iter().any(|f| f.contains("item")));
    assert!(info_resp.fields.iter().any(|f| f.contains("qty")));

    // 4. List tables via RPC
    let list_resp = service
        .list_tables(Request::new(ListTablesRequest {
            namespace: None,
            database: None,
        }))
        .await
        .expect("RPC list_tables failed")
        .into_inner();
    assert!(list_resp.tables.contains(&"inventory".to_string()));
}

#[tokio::test]
async fn test_grpc_network_roundtrip() {
    let db = init_memory_db("net_ns", "net_db")
        .await
        .expect("Failed to init in-memory db");
    let app_db = Arc::new(AppDb::Local(db));

    // Bind to port 0 to get an available port
    let addr = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };

    let config = Arc::new(AppConfig {
        gemini_api_key: "test_key".to_string(),
        model: "gemini-3.7-flash".to_string(),
        db: DatabaseConfig {
            endpoint: "mem://".to_string(),
            namespace: "net_ns".to_string(),
            database: "net_db".to_string(),
            username: None,
            password: None,
        },
        grpc: GrpcConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            ..Default::default()
        },
        ..Default::default()
    });

    let service = TablePopulatorServiceImpl::new(config.clone(), app_db.clone());
    let server = TablePopulatorServiceServer::new(service);

    // Spawn server in background task
    tokio::spawn(async move {
        Server::builder()
            .add_service(server)
            .serve(addr)
            .await
            .unwrap();
    });

    // Brief yield for server startup
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Connect with gRPC client
    let mut client = connect_client(&format!("http://{addr}"))
        .await
        .expect("Failed to connect gRPC client");

    // Execute DDL over gRPC
    let ddl_resp = client
        .execute_surreal_ql(ExecuteSurrealQlRequest {
            sql: "DEFINE TABLE servers SCHEMAFULL; DEFINE FIELD hostname ON TABLE servers TYPE string; DEFINE FIELD ip ON TABLE servers TYPE string;".to_string(),
            namespace: None,
            database: None,
        })
        .await
        .expect("execute_surreal_ql over gRPC failed")
        .into_inner();
    assert!(ddl_resp.success);

    // Set table comment
    set_table_comment(
        &*app_db,
        "servers",
        "List of critical production infrastructure servers",
    )
    .await
    .unwrap();

    // Query table info over gRPC
    let info_resp = client
        .get_table_info(GetTableInfoRequest {
            table_name: "servers".to_string(),
            namespace: None,
            database: None,
        })
        .await
        .expect("get_table_info over gRPC failed")
        .into_inner();
    assert_eq!(info_resp.table_name, "servers");
    assert_eq!(
        info_resp.comment,
        "List of critical production infrastructure servers"
    );
    assert!(info_resp.fields.iter().any(|f| f.contains("hostname")));

    // List tables over gRPC
    let list_resp = client
        .list_tables(ListTablesRequest {
            namespace: None,
            database: None,
        })
        .await
        .expect("list_tables over gRPC failed")
        .into_inner();
    assert!(list_resp.tables.contains(&"servers".to_string()));
}

#[tokio::test]
async fn test_populate_table_request_receipt_and_validation() {
    let db = init_memory_db("pop_ns", "pop_db")
        .await
        .expect("Failed to init db");
    let app_db = Arc::new(AppDb::Local(db));

    let config = Arc::new(AppConfig {
        gemini_api_key: "test_key".to_string(),
        model: "gemini-3.7-flash".to_string(),
        db: DatabaseConfig {
            endpoint: "mem://".to_string(),
            namespace: "pop_ns".to_string(),
            database: "pop_db".to_string(),
            username: None,
            password: None,
        },
        grpc: GrpcConfig {
            host: "127.0.0.1".to_string(),
            port: 50052,
            ..Default::default()
        },
        ..Default::default()
    });

    let service = TablePopulatorServiceImpl::new(config, app_db);

    // Empty prompt validation failure after receiving request
    let err = service
        .populate_table(Request::new(PopulateTableRequest {
            prompt: "".to_string(),
            table_name: "test_table".to_string(),
            namespace: None,
            database: None,
            define_table_sql: None,
            model: None,
            temperature: None,
            preamble: None,
            enable_grounding: None,
            omit_fields: Vec::new(),
            thinking_level: None,
            provider: None,
            base_url: None,
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert_eq!(err.message(), "Prompt must not be empty");

    // Empty table name validation failure after receiving request
    let err = service
        .populate_table(Request::new(PopulateTableRequest {
            prompt: "some prompt".to_string(),
            table_name: "  ".to_string(),
            namespace: None,
            database: None,
            define_table_sql: None,
            model: None,
            temperature: None,
            preamble: None,
            enable_grounding: None,
            omit_fields: Vec::new(),
            thinking_level: None,
            provider: None,
            base_url: None,
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert_eq!(err.message(), "Table name must not be empty");
}

#[tokio::test]
async fn test_schema_field_names_and_unlisted_field_sanitization() {
    let db = init_memory_db("test_sanitize_ns", "test_sanitize_db")
        .await
        .expect("Failed to init in-memory db");
    let app_db = Arc::new(AppDb::Local(db));

    // 1. Define a strict SCHEMAFULL table with only 'title' and 'url'
    let ddl = "DEFINE TABLE articles SCHEMAFULL; DEFINE FIELD title ON TABLE articles TYPE string; DEFINE FIELD url ON TABLE articles TYPE string;";
    execute_surrealql(&*app_db, ddl)
        .await
        .expect("Failed to execute DDL");

    // 2. Introspect schema summary and verify field_names
    let summary = get_table_schema_summary(&*app_db, "articles")
        .await
        .expect("Failed to get table schema summary");
    assert!(summary.field_names.contains(&"title".to_string()));
    assert!(summary.field_names.contains(&"url".to_string()));
    assert!(!summary.field_names.contains(&"incident_date".to_string()));

    // 3. Prepare records that contain an extraneous unlisted field 'incident_date'
    let raw_records = vec![json!({
        "title": "Article 1",
        "url": "https://example.com/1",
        "incident_date": "2024-01-01" // Extraneous field!
    })];

    // Raw insertion of this record directly into SCHEMAFULL table would fail:
    let direct_err = insert_dynamic_records(&*app_db, "articles", &raw_records).await;
    assert!(
        direct_err.is_err(),
        "SCHEMAFULL should reject unlisted field"
    );

    // 4. Sanitize records using field_names (same logic as in PopulateTable service)
    let allowed: std::collections::HashSet<&str> = summary
        .field_names
        .iter()
        .map(|s| s.as_str())
        .chain(std::iter::once("id"))
        .collect();

    let sanitized_records: Vec<serde_json::Value> = raw_records
        .into_iter()
        .map(|record| {
            if let serde_json::Value::Object(map) = record {
                let filtered: serde_json::Map<String, serde_json::Value> = map
                    .into_iter()
                    .filter(|(k, _)| allowed.contains(k.as_str()))
                    .collect();
                serde_json::Value::Object(filtered)
            } else {
                record
            }
        })
        .collect();

    // 5. Insert sanitized records — should succeed!
    let inserted = insert_dynamic_records(&*app_db, "articles", &sanitized_records)
        .await
        .expect("Sanitized records should insert successfully into SCHEMAFULL table");
    assert_eq!(inserted.len(), 1);
    assert_eq!(inserted[0]["title"], "Article 1");
    assert_eq!(inserted[0]["url"], "https://example.com/1");
    assert!(inserted[0].get("incident_date").is_none());
}

#[test]
fn test_sanitize_and_map_records_aliases() {
    let field_names = vec![
        "raw_text".to_string(),
        "incident_date".to_string(),
        "url".to_string(),
        "discovery_query".to_string(),
        "status".to_string(),
    ];

    let raw = vec![
        json!({
            "incident_title": "Bank robbery in Hamburg",
            "date": "2000-02-15",
            "link": "https://example.com/robbery",
            "query": "hamburg bank robbery 2000",
            "status": "Pending",
            "irrelevant_debug_field": 12345
        }),
        json!({
            "title": "Shooting report",
            "raw_text": "Detailed investigative police summary.",
            "crime_date": "2000-02-18",
            "source_url": "https://example.com/shooting",
            "status": "Extracted"
        }),
    ];

    let sanitized = sanitize_and_map_records(raw, &field_names);
    assert_eq!(sanitized.len(), 2);

    // First record:
    assert_eq!(sanitized[0]["raw_text"], "Bank robbery in Hamburg");
    assert_eq!(sanitized[0]["incident_date"], "2000-02-15");
    assert_eq!(sanitized[0]["url"], "https://example.com/robbery");
    assert_eq!(sanitized[0]["discovery_query"], "hamburg bank robbery 2000");
    assert_eq!(sanitized[0]["status"], "Pending");
    assert!(sanitized[0].get("incident_title").is_none());
    assert!(sanitized[0].get("date").is_none());
    assert!(sanitized[0].get("link").is_none());
    assert!(sanitized[0].get("query").is_none());
    assert!(sanitized[0].get("irrelevant_debug_field").is_none());

    // Second record:
    assert_eq!(
        sanitized[1]["raw_text"],
        "Shooting report: Detailed investigative police summary."
    );
    assert_eq!(sanitized[1]["incident_date"], "2000-02-18");
    assert_eq!(sanitized[1]["url"], "https://example.com/shooting");
    assert_eq!(sanitized[1]["status"], "Extracted");
    assert!(sanitized[1].get("title").is_none());
    assert!(sanitized[1].get("crime_date").is_none());
    assert!(sanitized[1].get("source_url").is_none());
}

#[tokio::test]
async fn test_omit_fields_filters_schema_and_preserves_defaults() {
    let db = init_memory_db("test_ns", "test_db")
        .await
        .expect("Failed to init in-memory database");
    let app_db = AppDb::Local(db);

    // 1. Define table with default values for lifecycle and timestamp fields
    let ddl = "DEFINE TABLE incident_source SCHEMAFULL; \
               DEFINE FIELD url ON TABLE incident_source TYPE string; \
               DEFINE FIELD raw_text ON TABLE incident_source TYPE string; \
               DEFINE FIELD status ON TABLE incident_source TYPE string DEFAULT 'Pending'; \
               DEFINE FIELD fetched_at ON TABLE incident_source TYPE datetime DEFAULT time::now();";
    execute_surrealql(&app_db, ddl)
        .await
        .expect("Failed to execute DDL");

    // 2. Introspect schema
    let summary = get_table_schema_summary(&app_db, "incident_source")
        .await
        .expect("Failed to introspect table");
    assert!(summary.field_names.contains(&"status".to_string()));
    assert!(summary.field_names.contains(&"fetched_at".to_string()));

    // 3. Define omit_fields list
    let omit_fields = vec!["status", "fetched_at"];
    let omit_set: std::collections::HashSet<&str> = omit_fields.into_iter().collect();

    let model_fields: Vec<String> = summary
        .fields
        .iter()
        .filter(|f| {
            let (name, _, _) = ai::grpc::extractor::parse_field_info(f);
            !omit_set.contains(name.as_str())
        })
        .cloned()
        .collect();

    let target_field_names: Vec<String> = summary
        .field_names
        .iter()
        .filter(|name| !omit_set.contains(name.as_str()))
        .cloned()
        .collect();

    assert_eq!(model_fields.len(), 2);
    assert!(!target_field_names.contains(&"status".to_string()));
    assert!(!target_field_names.contains(&"fetched_at".to_string()));
    assert!(target_field_names.contains(&"url".to_string()));
    assert!(target_field_names.contains(&"raw_text".to_string()));

    // 4. Model output that mistakenly includes "status": "Extracted"
    let raw_records = vec![json!({
        "url": "https://example.com/lead1",
        "raw_text": "Investigative lead about robbery",
        "status": "Extracted"
    })];

    // 5. Sanitization prunes the omitted "status" field
    let sanitized = sanitize_and_map_records(raw_records, &target_field_names);
    assert_eq!(sanitized.len(), 1);
    assert_eq!(sanitized[0]["url"], "https://example.com/lead1");
    assert_eq!(sanitized[0]["raw_text"], "Investigative lead about robbery");
    assert!(sanitized[0].get("status").is_none());

    // 6. Insertion into SurrealDB allows DEFAULT 'Pending' and DEFAULT time::now() to apply
    let inserted = insert_dynamic_records(&app_db, "incident_source", &sanitized)
        .await
        .expect("Should insert sanitized records into SCHEMAFULL table");
    assert_eq!(inserted.len(), 1);
    assert_eq!(inserted[0]["status"], "Pending");
    assert!(inserted[0].get("fetched_at").is_some());
}

#[tokio::test]
async fn test_populate_table_interval_validation_and_stepping() {
    let db = init_memory_db("test_ns", "test_db")
        .await
        .expect("Failed to init in-memory database");
    let app_db = Arc::new(AppDb::Local(db));

    let config = Arc::new(AppConfig {
        gemini_api_key: "".to_string(),
        model: "mock-model".to_string(),
        db: DatabaseConfig {
            endpoint: "mem://".to_string(),
            namespace: "test_ns".to_string(),
            database: "test_db".to_string(),
            username: Some("root".to_string()),
            password: Some("root".to_string()),
        },
        grpc: GrpcConfig {
            host: "127.0.0.1".to_string(),
            port: 50051,
            ..Default::default()
        },
        ..Default::default()
    });

    let service = TablePopulatorServiceImpl::new(config, app_db);

    // 1. Invalid interval returns error
    let err = service
        .populate_table_interval(Request::new(PopulateTableIntervalRequest {
            prompt: "Sample prompt".to_string(),
            table_name: "test_table".to_string(),
            interval: "invalid_interval".to_string(),
            start_date: "2000-01".to_string(),
            end_date: "2000-03".to_string(),
            namespace: None,
            database: None,
            define_table_sql: None,
            model: None,
            temperature: None,
            preamble: None,
            enable_grounding: None,
            omit_fields: Vec::new(),
            thinking_level: None,
            provider: None,
            base_url: None,
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("Unsupported interval"));

    // 2. Start date after end date returns error
    let err2 = service
        .populate_table_interval(Request::new(PopulateTableIntervalRequest {
            prompt: "Sample prompt".to_string(),
            table_name: "test_table".to_string(),
            interval: "monthly".to_string(),
            start_date: "2000-05".to_string(),
            end_date: "2000-01".to_string(),
            namespace: None,
            database: None,
            define_table_sql: None,
            model: None,
            temperature: None,
            preamble: None,
            enable_grounding: None,
            omit_fields: Vec::new(),
            thinking_level: None,
            provider: None,
            base_url: None,
        }))
        .await
        .unwrap_err();
    assert_eq!(err2.code(), tonic::Code::InvalidArgument);
    assert!(err2.message().contains("cannot be after end_date"));
}

#[tokio::test]
async fn test_gemini_3_model_enforcement_and_thinking_level() {
    let db = init_memory_db("test_ns", "test_db")
        .await
        .expect("Failed to init in-memory database");
    let _app_db = AppDb::Local(db);

    let config = Arc::new(AppConfig {
        gemini_api_key: "dummy_key".to_string(),
        model: "gemini-3.7-flash".to_string(),
        db: DatabaseConfig {
            endpoint: "mem://".to_string(),
            namespace: "test_ns".to_string(),
            database: "test_db".to_string(),
            username: None,
            password: None,
        },
        grpc: GrpcConfig {
            host: "127.0.0.1".to_string(),
            port: 50051,
            ..Default::default()
        },
        ..Default::default()
    });

    // 1. Calling extract_table_data with legacy model (gemini-2.5-flash) must be rejected
    let res = ai::grpc::extract_table_data(
        &config,
        "test prompt",
        "dummy_table",
        &[],
        Some("gemini"),
        Some("gemini-2.5-flash"),
        None,
        None,
        true,
        Some("low"),
        None,
    )
    .await;
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("only Gemini 3+ models"),
        "Unexpected error message: {err_msg}"
    );

    // 2. Calling extract_table_data with invalid thinking_level must be rejected
    let res = ai::grpc::extract_table_data(
        &config,
        "test prompt",
        "dummy_table",
        &[],
        Some("gemini"),
        Some("gemini-3.7-flash"),
        None,
        None,
        true,
        Some("ultra_high"),
        None,
    )
    .await;
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("Invalid thinking_level"),
        "Unexpected error message: {err_msg}"
    );

    // 3. Calling extract_table_data with unsupported provider must be rejected
    let res = ai::grpc::extract_table_data(
        &config,
        "test prompt",
        "dummy_table",
        &[],
        Some("unsupported_provider"),
        None,
        None,
        None,
        false,
        None,
        None,
    )
    .await;
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Unsupported provider"));

    // 4. Calling extract_table_data with Qwen but without Qwen API key must be rejected
    let res = ai::grpc::extract_table_data(
        &config,
        "test prompt",
        "dummy_table",
        &[],
        Some("qwen"),
        Some("qwen-plus"),
        None,
        None,
        false,
        None,
        Some("https://coding.dashscope.aliyuncs.com/v1"),
    )
    .await;
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Qwen API key is not configured"));
}

#[tokio::test]
async fn test_grpc_middleware_authentication_full_suite() {
    use ai::config::{GrpcAuthConfig, GrpcOauthConfig};
    use ai::grpc::{ClientAuth, connect_client_with_auth, create_auth_layer};
    use jsonwebtoken::{EncodingKey, Header, encode};

    let db = init_memory_db("auth_ns", "auth_db")
        .await
        .expect("Failed to init memory db");
    let app_db = Arc::new(AppDb::Local(db));

    // Bind on port 0 to get an ephemeral OS port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let jwt_secret = "test_super_secret_jwt_key_123456789";
    let admin_pass = "admin_super_secret_password_777";

    let auth_config = GrpcAuthConfig {
        enabled: Some(true),
        admin_user: "admin".to_string(),
        admin_password: Some(admin_pass.to_string()),
        oauth: GrpcOauthConfig {
            jwt_secret: Some(jwt_secret.to_string()),
            issuer: Some("test-auth-issuer".to_string()),
            audience: Some("test-grpc-api".to_string()),
            static_tokens: vec!["static_secret_token_123".to_string()],
            ..Default::default()
        },
    };

    let config = Arc::new(AppConfig {
        gemini_api_key: "test_key".to_string(),
        model: "gemini-3.7-flash".to_string(),
        db: DatabaseConfig {
            endpoint: "mem://".to_string(),
            namespace: "auth_ns".to_string(),
            database: "auth_db".to_string(),
            username: None,
            password: None,
        },
        grpc: GrpcConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            auth: auth_config.clone(),
        },
        ..Default::default()
    });

    let service = TablePopulatorServiceImpl::new(config.clone(), app_db.clone());
    let server = TablePopulatorServiceServer::new(service);
    let auth_layer = create_auth_layer(&auth_config).expect("Failed to create auth layer");

    // Spawn server with tonic-middleware layer attached
    tokio::spawn(async move {
        Server::builder()
            .layer(auth_layer)
            .add_service(server)
            .serve(addr)
            .await
            .unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(60)).await;
    let endpoint = format!("http://{addr}");

    // 1. Unauthenticated request MUST be rejected
    let mut unauth_client = connect_client(&endpoint).await.unwrap();
    let err = unauth_client
        .list_tables(ListTablesRequest {
            namespace: None,
            database: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
    assert!(err.message().contains("Missing authorization metadata"));

    // 2. Invalid Basic Auth password MUST be rejected
    let mut bad_basic_client = connect_client_with_auth(
        &endpoint,
        Some(ClientAuth::Basic {
            user: "admin".to_string(),
            pass: "wrong_password".to_string(),
        }),
    )
    .await
    .unwrap();
    let err = bad_basic_client
        .list_tables(ListTablesRequest {
            namespace: None,
            database: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
    assert!(err.message().contains("Invalid basic auth credentials"));

    // 3. Valid Basic Auth MUST succeed
    let mut good_basic_client = connect_client_with_auth(
        &endpoint,
        Some(ClientAuth::Basic {
            user: "admin".to_string(),
            pass: admin_pass.to_string(),
        }),
    )
    .await
    .unwrap();
    let resp = good_basic_client
        .list_tables(ListTablesRequest {
            namespace: None,
            database: None,
        })
        .await
        .expect("Valid Basic Auth request failed");
    assert!(resp.into_inner().tables.is_empty() || true);

    // 4. Invalid OAuth token MUST be rejected
    let mut bad_oauth_client = connect_client_with_auth(
        &endpoint,
        Some(ClientAuth::Bearer("invalid.jwt.token".to_string())),
    )
    .await
    .unwrap();
    let err = bad_oauth_client
        .list_tables(ListTablesRequest {
            namespace: None,
            database: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unauthenticated);

    // 5. Valid signed OAuth JWT MUST succeed
    let exp = (chrono::Utc::now() + chrono::Duration::hours(2)).timestamp() as usize;
    let claims = serde_json::json!({
        "sub": "oauth_user_99",
        "iss": "test-auth-issuer",
        "aud": "test-grpc-api",
        "exp": exp,
    });
    let valid_jwt = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .unwrap();

    let mut good_oauth_client =
        connect_client_with_auth(&endpoint, Some(ClientAuth::Bearer(valid_jwt)))
            .await
            .unwrap();
    let resp = good_oauth_client
        .list_tables(ListTablesRequest {
            namespace: None,
            database: None,
        })
        .await
        .expect("Valid OAuth JWT request failed");
    let _ = resp.into_inner();

    // 6. Valid static Bearer token MUST succeed
    let mut static_oauth_client = connect_client_with_auth(
        &endpoint,
        Some(ClientAuth::Bearer("static_secret_token_123".to_string())),
    )
    .await
    .unwrap();
    let resp = static_oauth_client
        .list_tables(ListTablesRequest {
            namespace: None,
            database: None,
        })
        .await
        .expect("Valid static token request failed");
    let _ = resp.into_inner();
}

#[test]
fn test_admin_password_from_env_var() {
    unsafe {
        std::env::set_var("ADMIN_PASSWORD", "super_env_admin_pass_99");
        std::env::set_var("ADMIN_USER", "custom_admin");
    }

    let config = AppConfig::load_from_config().expect("Failed to load config");
    assert_eq!(
        config.grpc.auth.admin_password.as_deref(),
        Some("super_env_admin_pass_99")
    );
    assert_eq!(config.grpc.auth.admin_user, "custom_admin");
    assert!(config.grpc.auth.is_active());

    unsafe {
        std::env::remove_var("ADMIN_PASSWORD");
        std::env::remove_var("ADMIN_USER");
    }
}

#[tokio::test]
async fn test_oauth_startup_check_integration() {
    use ai::config::{GrpcAuthConfig, GrpcOauthConfig};
    use ai::grpc::check_oauth_at_startup;

    // 1. Static tokens startup check
    let mut static_auth = GrpcAuthConfig {
        enabled: Some(true),
        oauth: GrpcOauthConfig {
            static_tokens: vec!["dev_token_abc".to_string()],
            ..Default::default()
        },
        ..Default::default()
    };

    let report = check_oauth_at_startup(&mut static_auth)
        .await
        .expect("Static token check should succeed")
        .expect("Report should be present");
    assert_eq!(report.issuer, None);

    // 2. Disabled check
    let mut disabled_auth = GrpcAuthConfig {
        enabled: Some(false),
        oauth: GrpcOauthConfig {
            static_tokens: vec!["dev_token_abc".to_string()],
            ..Default::default()
        },
        ..Default::default()
    };
    let report_none = check_oauth_at_startup(&mut disabled_auth)
        .await
        .expect("Disabled auth check should return Ok(None)");
    assert!(report_none.is_none());

    // 3. check_on_startup = false override
    let mut bypassed_auth = GrpcAuthConfig {
        enabled: Some(true),
        oauth: GrpcOauthConfig {
            check_on_startup: Some(false),
            static_tokens: vec!["dev_token_abc".to_string()],
            ..Default::default()
        },
        ..Default::default()
    };
    let report_bypassed = check_oauth_at_startup(&mut bypassed_auth)
        .await
        .expect("Bypassed auth check should return Ok(None)");
    assert!(report_bypassed.is_none());
}

#[tokio::test]
async fn test_ai_admin_vault_secret_and_basic_auth() {
    use ai::grpc::AuthValidator;
    use base64::Engine;

    // 1. Verify apply_secret_data picks up "ai_admin"
    let mut config = AppConfig::default();
    let secret_json = serde_json::json!({
        "ai_admin": "super_secret_pw_123",
        "gemini_api_key": "test_gemini"
    });
    let count = config.apply_secret_data(secret_json.as_object().unwrap());
    assert!(count >= 2);
    assert_eq!(
        config.grpc.auth.admin_password.as_deref(),
        Some("super_secret_pw_123")
    );
    assert!(config.grpc.auth.is_active());

    // 2. Verify AuthValidator accepts both "admin" and "ai_admin" with the password
    let validator = AuthValidator::new(&config.grpc.auth);

    // a) User "admin"
    let basic_admin = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("admin:super_secret_pw_123")
    );
    let auth_identity = validator
        .validate_auth_header(Some(&basic_admin))
        .await
        .expect("Valid basic auth for admin should succeed");
    assert!(matches!(auth_identity, ai::grpc::AuthIdentity::Admin(u) if u == "admin"));

    // b) User "ai_admin"
    let basic_ai_admin = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("ai_admin:super_secret_pw_123")
    );
    let auth_identity_2 = validator
        .validate_auth_header(Some(&basic_ai_admin))
        .await
        .expect("Valid basic auth for ai_admin should succeed");
    assert!(matches!(auth_identity_2, ai::grpc::AuthIdentity::Admin(u) if u == "ai_admin"));

    // c) Invalid password
    let bad_pass = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("admin:wrong_password")
    );
    let err = validator
        .validate_auth_header(Some(&bad_pass))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unauthenticated);

    // d) Invalid username
    let bad_user = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("unknown_user:super_secret_pw_123")
    );
    let err = validator
        .validate_auth_header(Some(&bad_user))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
}

#[tokio::test]
async fn test_grpc_reflection_v1_and_v1alpha() {
    use std::net::TcpListener;
    use tokio_stream::wrappers::ReceiverStream;
    use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient as ReflectionClientV1;
    use tonic_reflection::pb::v1::{
        ServerReflectionRequest as ReflectionReqV1,
        server_reflection_request::MessageRequest as MessageReqV1,
        server_reflection_response::MessageResponse as MessageRespV1,
    };
    use tonic_reflection::pb::v1alpha::server_reflection_client::ServerReflectionClient as ReflectionClientV1Alpha;
    use tonic_reflection::pb::v1alpha::{
        ServerReflectionRequest as ReflectionReqV1Alpha,
        server_reflection_request::MessageRequest as MessageReqV1Alpha,
        server_reflection_response::MessageResponse as MessageRespV1Alpha,
    };

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let db = init_memory_db("refl_ns", "refl_db")
        .await
        .expect("Failed to init memory db");
    let app_db = Arc::new(AppDb::Local(db));
    let config = Arc::new(AppConfig {
        gemini_api_key: "test_key".to_string(),
        model: "gemini-3.7-flash".to_string(),
        db: DatabaseConfig {
            endpoint: "mem://".to_string(),
            namespace: "refl_ns".to_string(),
            database: "refl_db".to_string(),
            username: None,
            password: None,
        },
        grpc: GrpcConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            ..Default::default()
        },
        ..Default::default()
    });

    let service = TablePopulatorServiceImpl::new(config.clone(), app_db.clone());
    let server = TablePopulatorServiceServer::new(service);

    let reflection_v1 = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(ai::grpc::FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("Failed to build v1 reflection service");
    let reflection_v1alpha = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(ai::grpc::FILE_DESCRIPTOR_SET)
        .build_v1alpha()
        .expect("Failed to build v1alpha reflection service");

    tokio::spawn(async move {
        Server::builder()
            .add_service(reflection_v1)
            .add_service(reflection_v1alpha)
            .add_service(server)
            .serve(addr)
            .await
            .unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let channel = tonic::transport::Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("Failed to connect channel");

    // 1. Test v1 Reflection
    {
        let mut client = ReflectionClientV1::new(channel.clone());
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(ReflectionReqV1 {
            host: String::new(),
            message_request: Some(MessageReqV1::ListServices(String::new())),
        })
        .await
        .unwrap();
        drop(tx);

        let mut resp_stream = client
            .server_reflection_info(ReceiverStream::new(rx))
            .await
            .expect("v1 server_reflection_info call failed")
            .into_inner();

        let resp = resp_stream
            .message()
            .await
            .expect("Stream read failed")
            .expect("Expected v1 reflection response");

        match resp.message_response {
            Some(MessageRespV1::ListServicesResponse(services_resp)) => {
                let service_names: Vec<String> =
                    services_resp.service.into_iter().map(|s| s.name).collect();
                assert!(
                    service_names
                        .iter()
                        .any(|s| s == "table_populator.TablePopulatorService"),
                    "table_populator.TablePopulatorService should be in listed v1 services: {:?}",
                    service_names
                );
                assert!(
                    service_names
                        .iter()
                        .any(|s| s == "grpc.reflection.v1.ServerReflection"),
                    "grpc.reflection.v1.ServerReflection should be in listed v1 services: {:?}",
                    service_names
                );
            }
            other => panic!("Unexpected response for v1 ListServices: {:?}", other),
        }
    }

    // 2. Test v1alpha Reflection
    {
        let mut client = ReflectionClientV1Alpha::new(channel);
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(ReflectionReqV1Alpha {
            host: String::new(),
            message_request: Some(MessageReqV1Alpha::ListServices(String::new())),
        })
        .await
        .unwrap();
        drop(tx);

        let mut resp_stream = client
            .server_reflection_info(ReceiverStream::new(rx))
            .await
            .expect("v1alpha server_reflection_info call failed")
            .into_inner();

        let resp = resp_stream
            .message()
            .await
            .expect("Stream read failed")
            .expect("Expected v1alpha reflection response");

        match resp.message_response {
            Some(MessageRespV1Alpha::ListServicesResponse(services_resp)) => {
                let service_names: Vec<String> =
                    services_resp.service.into_iter().map(|s| s.name).collect();
                assert!(
                    service_names
                        .iter()
                        .any(|s| s == "table_populator.TablePopulatorService"),
                    "table_populator.TablePopulatorService should be in listed v1alpha services: {:?}",
                    service_names
                );
                assert!(
                    service_names
                        .iter()
                        .any(|s| s == "grpc.reflection.v1alpha.ServerReflection"),
                    "grpc.reflection.v1alpha.ServerReflection should be in listed v1alpha services: {:?}",
                    service_names
                );
            }
            other => panic!("Unexpected response for v1alpha ListServices: {:?}", other),
        }
    }
}

#[tokio::test]
async fn test_fact_check_entries_validation() {
    let db = init_memory_db("fc_ns", "fc_db").await.unwrap();
    let app_db = Arc::new(AppDb::Local(db));
    let config = Arc::new(AppConfig::default());
    let service = TablePopulatorServiceImpl::new(config, app_db);

    // 1. Empty table name
    let err = service
        .fact_check_entries(Request::new(FactCheckEntriesRequest {
            table_name: "".to_string(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("Table name must not be empty"));

    // 2. Empty filters
    let err = service
        .fact_check_entries(Request::new(FactCheckEntriesRequest {
            table_name: "crimes".to_string(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(
        err.message()
            .contains("At least one filter must be provided")
    );
}

#[tokio::test]
async fn test_fact_check_entries_query_filtering_and_status() {
    let db = init_memory_db("fc_filter_ns", "fc_filter_db")
        .await
        .unwrap();
    let app_db = Arc::new(AppDb::Local(db));

    // 1. Define table schema
    let ddl = "DEFINE TABLE incidents SCHEMAFULL; \
               DEFINE FIELD incident_date ON TABLE incidents TYPE datetime; \
               DEFINE FIELD fetched_at ON TABLE incidents TYPE datetime; \
               DEFINE FIELD status ON TABLE incidents TYPE string DEFAULT 'Pending'; \
               DEFINE FIELD raw_text ON TABLE incidents TYPE string;";
    execute_surrealql(&*app_db, ddl).await.unwrap();

    // 2. Insert records with incident_date, fetched_at, and status
    let insert_sql = "INSERT INTO incidents [
        { incident_date: <datetime>'2024-01-15T00:00:00Z', fetched_at: <datetime>'2024-02-01T10:00:00Z', status: 'Pending', raw_text: 'Robbery in Berlin' },
        { incident_date: <datetime>'2024-06-20T00:00:00Z', fetched_at: <datetime>'2024-07-01T12:00:00Z', status: 'Pending', raw_text: 'Burglary in Munich' },
        { incident_date: <datetime>'2024-11-05T00:00:00Z', fetched_at: <datetime>'2024-11-10T08:00:00Z', status: 'Checked', raw_text: 'Theft in Hamburg' }
    ];";
    let inserted_val = execute_surrealql(&*app_db, insert_sql).await.unwrap();

    let config = Arc::new(AppConfig {
        gemini_api_key: "".to_string(),
        ..Default::default()
    });
    let service = TablePopulatorServiceImpl::new(config, app_db.clone());

    // 3. Filter by incident_date date range (Jan to Mar 2024) -> only record 1 (Berlin)
    let resp = service
        .fact_check_entries(Request::new(FactCheckEntriesRequest {
            table_name: "incidents".to_string(),
            start_date: Some("2024-01-01".to_string()),
            end_date: Some("2024-03-31".to_string()),
            date_field: Some("incident_date".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp.total_checked, 1);
    assert!(resp.results[0].record_json.contains("Berlin"));

    // 4. Filter by fetched_at date range (July 2024) -> only record 2 (Munich)
    let resp = service
        .fact_check_entries(Request::new(FactCheckEntriesRequest {
            table_name: "incidents".to_string(),
            start_date: Some("2024-07-01".to_string()),
            end_date: Some("2024-07-31".to_string()),
            date_field: Some("fetched_at".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp.total_checked, 1);
    assert!(resp.results[0].record_json.contains("Munich"));

    // 5. Filter by status 'Checked' -> only record 3 (Hamburg)
    let resp = service
        .fact_check_entries(Request::new(FactCheckEntriesRequest {
            table_name: "incidents".to_string(),
            status_filter: Some("Checked".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp.total_checked, 1);
    assert!(resp.results[0].record_json.contains("Hamburg"));

    // 6. Filter by explicit record_ids
    let id0 = match &inserted_val {
        serde_json::Value::Array(arr) => arr[0]["id"].as_str().unwrap().to_string(),
        _ => panic!("Expected array from execute_surrealql"),
    };
    let resp = service
        .fact_check_entries(Request::new(FactCheckEntriesRequest {
            table_name: "incidents".to_string(),
            record_ids: vec![id0],
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp.total_checked, 1);
    assert!(resp.results[0].record_json.contains("Berlin"));
}

#[tokio::test]
async fn test_fact_check_entries_stream() {
    let db = init_memory_db("fc_stream_ns", "fc_stream_db")
        .await
        .unwrap();
    let app_db = Arc::new(AppDb::Local(db));

    let ddl = "DEFINE TABLE alerts SCHEMAFULL; \
               DEFINE FIELD incident_date ON TABLE alerts TYPE datetime; \
               DEFINE FIELD status ON TABLE alerts TYPE string DEFAULT 'Pending'; \
               DEFINE FIELD msg ON TABLE alerts TYPE string;";
    execute_surrealql(&*app_db, ddl).await.unwrap();

    let insert_sql = "INSERT INTO alerts [
        { incident_date: <datetime>'2025-01-01T00:00:00Z', status: 'Pending', msg: 'Alert 1' },
        { incident_date: <datetime>'2025-01-02T00:00:00Z', status: 'Pending', msg: 'Alert 2' }
    ];";
    execute_surrealql(&*app_db, insert_sql).await.unwrap();

    let config = Arc::new(AppConfig::default());
    let service = TablePopulatorServiceImpl::new(config, app_db);

    let mut stream = service
        .fact_check_entries_stream(Request::new(FactCheckEntriesRequest {
            table_name: "alerts".to_string(),
            status_filter: Some("Pending".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();

    use tokio_stream::StreamExt;
    let mut count = 0;
    while let Some(res) = stream.next().await {
        let item = res.unwrap();
        count += 1;
        assert!(!item.record_id.is_empty());
        assert!(!item.record_json.is_empty());
    }
    assert_eq!(count, 2);
}

#[test]
fn test_fact_check_outcome_parsing_and_status_assignment() {
    // 1. Confirmed verdict -> Checked
    let raw_confirmed = r#"{
        "verdict": "confirmed",
        "explanation": "Incident verified through official police records.",
        "sources": ["https://police.de/report/123"]
    }"#;
    let parsed = parse_json_response(raw_confirmed).unwrap();
    let first = &parsed[0];
    let verdict = first["verdict"].as_str().unwrap();
    let status = if verdict == "confirmed" {
        "Checked"
    } else {
        "Discarded_Irrelevant"
    };
    assert_eq!(verdict, "confirmed");
    assert_eq!(status, "Checked");

    // 2. Disputed verdict -> Discarded_Irrelevant
    let raw_disputed = r#"{
        "verdict": "disputed",
        "explanation": "Official court statement refutes this claim as fabricated.",
        "sources": ["https://factcheck.org/claim/456"]
    }"#;
    let parsed = parse_json_response(raw_disputed).unwrap();
    let first = &parsed[0];
    let verdict = first["verdict"].as_str().unwrap();
    let status = if verdict == "confirmed" {
        "Checked"
    } else {
        "Discarded_Irrelevant"
    };
    assert_eq!(verdict, "disputed");
    assert_eq!(status, "Discarded_Irrelevant");

    // 3. Unverifiable verdict -> Discarded_Irrelevant
    let raw_unverifiable = r#"{
        "verdict": "unverifiable",
        "explanation": "No reputable sources found for this report.",
        "sources": []
    }"#;
    let parsed = parse_json_response(raw_unverifiable).unwrap();
    let first = &parsed[0];
    let verdict = first["verdict"].as_str().unwrap();
    let status = if verdict == "confirmed" {
        "Checked"
    } else {
        "Discarded_Irrelevant"
    };
    assert_eq!(verdict, "unverifiable");
    assert_eq!(status, "Discarded_Irrelevant");
}

#[tokio::test]
async fn test_fact_check_db_status_update_behavior() {
    let db = init_memory_db("fc_update_ns", "fc_update_db")
        .await
        .unwrap();
    let app_db = Arc::new(AppDb::Local(db));

    let ddl = "DEFINE TABLE entries SCHEMAFULL; \
               DEFINE FIELD status ON TABLE entries TYPE string DEFAULT 'Pending'; \
               DEFINE FIELD title ON TABLE entries TYPE string;";
    execute_surrealql(&*app_db, ddl).await.unwrap();

    let insert_sql = "INSERT INTO entries [{ title: 'Entry A', status: 'Pending' }, { title: 'Entry B', status: 'Pending' }];";
    let inserted = execute_surrealql(&*app_db, insert_sql).await.unwrap();
    let id_a = inserted[0]["id"].as_str().unwrap();
    let id_b = inserted[1]["id"].as_str().unwrap();

    // Update status to 'Checked' for A
    execute_surrealql(&*app_db, &format!("UPDATE {id_a} SET status = 'Checked';"))
        .await
        .unwrap();

    // Update status to 'Discarded_Irrelevant' for B
    execute_surrealql(
        &*app_db,
        &format!("UPDATE {id_b} SET status = 'Discarded_Irrelevant';"),
    )
    .await
    .unwrap();

    // Verify status was persisted in SurrealDB
    let q_a = execute_surrealql(&*app_db, &format!("SELECT status FROM {id_a};"))
        .await
        .unwrap();
    assert_eq!(q_a[0]["status"], "Checked");

    let q_b = execute_surrealql(&*app_db, &format!("SELECT status FROM {id_b};"))
        .await
        .unwrap();
    assert_eq!(q_b[0]["status"], "Discarded_Irrelevant");
}
