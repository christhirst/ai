use ai::config::{AppConfig, DatabaseConfig, GrpcConfig};
use ai::db::{
    execute_surrealql, get_defined_tables, get_table_schema_summary, init_memory_db,
    insert_dynamic_records, set_table_comment, AppDb,
};
use ai::grpc::{
    connect_client, parse_json_response, sanitize_and_map_records, ExecuteSurrealQlRequest,
    GetTableInfoRequest, ListTablesRequest, PopulateTableIntervalRequest, PopulateTableRequest,
    TablePopulatorService, TablePopulatorServiceImpl, TablePopulatorServiceServer,
};
use serde_json::json;
use std::sync::Arc;
use tonic::transport::Server;
use tonic::Request;

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
    execute_surrealql(&app_db, ddl).await.expect("Failed to execute DDL");

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
        temperature: Some(0.0),
        preamble: None,
        variant: ai::config::ExecutionVariant::Typed,
        prompt: Default::default(),
        prompt_typed: Default::default(),
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
        },
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
    set_table_comment(&*app_db, "inventory", "Track inventory levels for warehouse A")
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
        temperature: Some(0.0),
        preamble: None,
        variant: ai::config::ExecutionVariant::Typed,
        prompt: Default::default(),
        prompt_typed: Default::default(),
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
        },
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
    set_table_comment(&*app_db, "servers", "List of critical production infrastructure servers")
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
    assert_eq!(info_resp.comment, "List of critical production infrastructure servers");
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
        temperature: Some(0.0),
        preamble: None,
        variant: ai::config::ExecutionVariant::Typed,
        prompt: Default::default(),
        prompt_typed: Default::default(),
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
        },
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
    execute_surrealql(&*app_db, ddl).await.expect("Failed to execute DDL");

    // 2. Introspect schema summary and verify field_names
    let summary = get_table_schema_summary(&*app_db, "articles")
        .await
        .expect("Failed to get table schema summary");
    assert!(summary.field_names.contains(&"title".to_string()));
    assert!(summary.field_names.contains(&"url".to_string()));
    assert!(!summary.field_names.contains(&"incident_date".to_string()));

    // 3. Prepare records that contain an extraneous unlisted field 'incident_date'
    let raw_records = vec![
        json!({
            "title": "Article 1",
            "url": "https://example.com/1",
            "incident_date": "2024-01-01" // Extraneous field!
        }),
    ];

    // Raw insertion of this record directly into SCHEMAFULL table would fail:
    let direct_err = insert_dynamic_records(&*app_db, "articles", &raw_records).await;
    assert!(direct_err.is_err(), "SCHEMAFULL should reject unlisted field");

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
    execute_surrealql(&app_db, ddl).await.expect("Failed to execute DDL");

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
        temperature: Some(0.0),
        preamble: None,
        variant: ai::config::ExecutionVariant::Typed,
        prompt: Default::default(),
        prompt_typed: Default::default(),
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
        },
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
        }))
        .await
        .unwrap_err();
    assert_eq!(err2.code(), tonic::Code::InvalidArgument);
    assert!(err2.message().contains("cannot be after end_date"));
}




