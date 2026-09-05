use ai::config::{AppConfig, DatabaseConfig, GrpcConfig};
use ai::db::{
    execute_surrealql, get_defined_tables, get_table_schema_summary, init_memory_db,
    insert_dynamic_records, set_table_comment, AppDb,
};
use ai::grpc::{
    connect_client, parse_json_response, ExecuteSurrealQlRequest, GetTableInfoRequest,
    ListTablesRequest, TablePopulatorService, TablePopulatorServiceImpl,
    TablePopulatorServiceServer,
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
