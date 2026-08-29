use ai::config::{AppConfig, DatabaseConfig};
use ai::db::{
    AppDb, LocalDb, create_gdp_record, create_gdp_records, create_homecide_record,
    create_homecide_records, delete_all_gdp_records, delete_all_homecides,
    delete_gdp_records_by_year, delete_homecides_by_citizenship, delete_homecides_by_city,
    get_all_gdp_records, get_all_homecides, get_db_info, get_defined_tables,
    get_gdp_records_by_year, get_homecides_by_citizenship, get_homecides_by_city,
    get_homecides_by_weapon, get_table_info, init_db_from_config, init_memory_db,
};
use ai::{GdpRecord, Homecides};

#[tokio::test]
async fn test_db_initialization() {
    let db: LocalDb = init_memory_db("test_ns", "test_db")
        .await
        .expect("Failed to init in-memory database");

    let config = DatabaseConfig {
        // TODO: Refactor
        endpoint: "mem://".to_string(),
        namespace: "test_custom_ns".to_string(),
        database: "test_custom_db".to_string(),
        username: None,
        password: None,
    };

    let config_db: AppDb = init_db_from_config(&config)
        .await
        .expect("Failed to init db from config");

    let gdp_initial: Vec<GdpRecord> = get_all_gdp_records(&db).await.unwrap();
    assert!(gdp_initial.is_empty());

    let gdp_custom_initial: Vec<GdpRecord> = get_all_gdp_records(&config_db).await.unwrap();
    assert!(gdp_custom_initial.is_empty());
}

#[test]
fn test_homecides_json_serde_roundtrip() {
    let json_data = r#"{
        "citiy": "Berlin",
        "Citizenship": "German",
        "Date": "2026-01-15T00:00:00Z",
        "weapon": "Knife",
        "Prison_time": "15y",
        "Other_sentence": "None"
    }"#;
    let res: Result<Homecides, _> = serde_json::from_str(json_data);
    assert!(res.is_ok(), "Failed to deserialize JSON: {:?}", res.err());
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[tokio::test]
async fn test_app_config_db_loading() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        std::env::remove_var("SURREAL_URL");
        std::env::remove_var("SURREALDB_URL");
        std::env::remove_var("DB_ENDPOINT");
        std::env::remove_var("SURREAL_PASS");
        std::env::remove_var("SURREALDB_PASS");
        std::env::remove_var("DB_PASSWORD");
        std::env::remove_var("SURREAL_USER");
        std::env::remove_var("SURREALDB_USER");
        std::env::remove_var("DB_USERNAME");
        std::env::remove_var("SURREAL_NS");
        std::env::remove_var("SURREALDB_NS");
        std::env::remove_var("DB_NAMESPACE");
        std::env::remove_var("SURREAL_DB");
        std::env::remove_var("SURREALDB_DB");
        std::env::remove_var("DB_DATABASE");
        if std::env::var("GEMINI_API_KEY").is_err() && std::env::var("APP_GEMINI_API_KEY").is_err() {
            std::env::set_var("GEMINI_API_KEY", "test_key");
        }
    }
    let config = AppConfig::load().expect("Failed to load AppConfig");
    assert_eq!(config.db.namespace, "data");
    assert_eq!(config.db.database, "ai");

    let mem_config = DatabaseConfig {
        endpoint: "mem://".to_string(),
        namespace: "data".to_string(),
        database: "ai".to_string(),
        username: None,
        password: None,
    };
    let db = init_db_from_config(&mem_config)
        .await
        .expect("Failed to connect using memory config");

    let record = GdpRecord {
        year: "2024".to_string(),
        gdp: 4.45,
    };
    create_gdp_record(&db, &record).await.unwrap();
    let records = get_all_gdp_records(&db).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].year, "2024");
}

#[tokio::test]
async fn test_gdp_record_crud_lifecycle() {
    let db = init_memory_db("test_gdp_ns", "test_gdp_db")
        .await
        .expect("Failed to init db");

    // 1. Create single record
    let record_1990 = GdpRecord {
        year: "1990".to_string(),
        gdp: 1.77,
    };
    let created = create_gdp_record(&db, &record_1990)
        .await
        .expect("Failed to insert single record");
    assert!(created.is_some());
    assert_eq!(created.unwrap().year, "1990");

    // 2. Create bulk records
    let bulk_records = vec![
        GdpRecord {
            year: "1995".to_string(),
            gdp: 2.59,
        },
        GdpRecord {
            year: "2000".to_string(),
            gdp: 1.95,
        },
        GdpRecord {
            year: "2000".to_string(),
            gdp: 1.96, // duplicate year for query test
        },
    ];
    let created_bulk = create_gdp_records(&db, &bulk_records)
        .await
        .expect("Failed to insert bulk records");
    assert_eq!(created_bulk.len(), 3);

    // 3. Read all records
    let all_records = get_all_gdp_records(&db)
        .await
        .expect("Failed to fetch all records");
    assert_eq!(all_records.len(), 4);

    // 4. Query by year
    let records_2000 = get_gdp_records_by_year(&db, "2000")
        .await
        .expect("Failed to query by year");
    assert_eq!(records_2000.len(), 2);
    assert!(records_2000.iter().all(|r| r.year == "2000"));

    let records_1990 = get_gdp_records_by_year(&db, "1990")
        .await
        .expect("Failed to query by year");
    assert_eq!(records_1990.len(), 1);
    assert_eq!(records_1990[0].gdp, 1.77);

    // 5. Delete by year
    let deleted_2000 = delete_gdp_records_by_year(&db, "2000")
        .await
        .expect("Failed to delete records by year");
    assert_eq!(deleted_2000.len(), 2);

    let remaining_2000 = get_gdp_records_by_year(&db, "2000")
        .await
        .expect("Failed to query 2000 after delete");
    assert!(remaining_2000.is_empty());

    let remaining_all = get_all_gdp_records(&db)
        .await
        .expect("Failed to fetch remaining records");
    assert_eq!(remaining_all.len(), 2);

    // 6. Delete all
    let deleted_all = delete_all_gdp_records(&db)
        .await
        .expect("Failed to delete all GDP records");
    assert_eq!(deleted_all.len(), 2);

    let final_check = get_all_gdp_records(&db)
        .await
        .expect("Failed to fetch records after purge");
    assert!(final_check.is_empty());
}

#[tokio::test]
async fn test_homecides_crud_lifecycle() {
    let db = init_memory_db("test_homecides_ns", "test_homecides_db")
        .await
        .expect("Failed to init db");

    // 1. Create single record
    let record1 = Homecides {
        citiy: "Berlin".to_string(),
        Citizenship: "German".to_string(),
        Date: "2026-01-15T10:00:00Z".parse().unwrap(),
        weapon: "Knife".to_string(),
        Prison_time: "15y".parse().unwrap(),
        Other_sentence: "None".to_string(),
    };
    let created = create_homecide_record(&db, &record1)
        .await
        .expect("Failed to insert single record");
    assert!(created.is_some());
    let created_unwrapped = created.unwrap();
    assert_eq!(created_unwrapped.citiy, "Berlin");
    assert_eq!(created_unwrapped.Citizenship, "German");
    assert_eq!(created_unwrapped.weapon, "Knife");

    // 2. Create bulk records
    let bulk_records = vec![
        Homecides {
            citiy: "Berlin".to_string(),
            Citizenship: "Polish".to_string(),
            Date: "2026-01-18T14:30:00Z".parse().unwrap(),
            weapon: "Firearm".to_string(),
            Prison_time: "20y".parse().unwrap(),
            Other_sentence: "Fine 5000 EUR".to_string(),
        },
        Homecides {
            citiy: "Hamburg".to_string(),
            Citizenship: "German".to_string(),
            Date: "2026-01-20T21:00:00Z".parse().unwrap(),
            weapon: "Knife".to_string(),
            Prison_time: "12y".parse().unwrap(),
            Other_sentence: "Probation".to_string(),
        },
        Homecides {
            citiy: "Berlin".to_string(),
            Citizenship: "German".to_string(),
            Date: "2026-01-25T08:00:00Z".parse().unwrap(),
            weapon: "Blunt Object".to_string(),
            Prison_time: "10y".parse().unwrap(),
            Other_sentence: "None".to_string(),
        },
    ];
    let created_bulk = create_homecide_records(&db, &bulk_records)
        .await
        .expect("Failed to insert bulk records");
    assert_eq!(created_bulk.len(), 3);

    // 3. Read all records
    let all_records = get_all_homecides(&db)
        .await
        .expect("Failed to get all homecides");
    assert_eq!(all_records.len(), 4);

    // 4. Query by city
    let berlin_records = get_homecides_by_city(&db, "Berlin")
        .await
        .expect("Failed to get homecides by city");
    assert_eq!(berlin_records.len(), 3);
    assert!(berlin_records.iter().all(|h| h.citiy == "Berlin"));

    // 5. Query by citizenship
    let german_records = get_homecides_by_citizenship(&db, "German")
        .await
        .expect("Failed to filter by citizenship");
    assert_eq!(german_records.len(), 3);
    assert!(german_records.iter().all(|h| h.Citizenship == "German"));

    // 6. Query by weapon
    let knife_records = get_homecides_by_weapon(&db, "Knife")
        .await
        .expect("Failed to filter by weapon");
    assert_eq!(knife_records.len(), 2);
    assert!(knife_records.iter().all(|h| h.weapon == "Knife"));

    // 7. Delete by city
    let deleted_hamburg = delete_homecides_by_city(&db, "Hamburg")
        .await
        .expect("Failed to delete by city");
    assert_eq!(deleted_hamburg.len(), 1);

    let remaining_hamburg = get_homecides_by_city(&db, "Hamburg")
        .await
        .expect("Failed to query hamburg after delete");
    assert!(remaining_hamburg.is_empty());

    // 8. Delete by citizenship
    let deleted_polish = delete_homecides_by_citizenship(&db, "Polish")
        .await
        .expect("Failed to delete by citizenship");
    assert_eq!(deleted_polish.len(), 1);

    let remaining_all = get_all_homecides(&db)
        .await
        .expect("Failed to get all records after deletion");
    assert_eq!(remaining_all.len(), 2);

    // 9. Delete all
    let deleted_all = delete_all_homecides(&db)
        .await
        .expect("Failed to delete all records");
    assert_eq!(deleted_all.len(), 2);

    let final_check = get_all_homecides(&db)
        .await
        .expect("Failed to fetch records after purge");
    assert!(final_check.is_empty());
}

#[tokio::test]
async fn test_db_schema_introspection() {
    let db = init_memory_db("data", "ai")
        .await
        .expect("Failed to init db");

    let record = Homecides {
        citiy: "Berlin".to_string(),
        Citizenship: "German".to_string(),
        Date: "2026-01-01T00:00:00Z".parse().unwrap(),
        weapon: "Knife".to_string(),
        Prison_time: "10y".parse().unwrap(),
        Other_sentence: "None".to_string(),
    };
    create_homecide_record(&db, &record).await.unwrap();

    let db_info = get_db_info(&db).await.expect("Failed to query INFO FOR DB");
    assert!(!db_info.is_null());

    let tables = get_defined_tables(&db)
        .await
        .expect("Failed to query defined tables");
    assert!(tables.contains(&"homecides".to_string()));

    let table_info = get_table_info(&db, "homecides")
        .await
        .expect("Failed to query table info");
    assert!(!table_info.is_null());
}

#[tokio::test]
async fn test_env_var_credential_overrides() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        std::env::set_var("SURREAL_PASS", "test_env_password_123");
        std::env::set_var("SURREAL_USER", "test_env_user");
        std::env::set_var("SURREAL_URL", "ws://127.0.0.1:8000");
        std::env::set_var("SURREAL_NS", "data");
        std::env::set_var("SURREAL_DB", "ai");
        std::env::set_var("GEMINI_API_KEY", "test_env_gemini_key");
    }

    let config = AppConfig::load().expect("Failed to load config with env vars");
    assert_eq!(config.gemini_api_key, "test_env_gemini_key");
    assert_eq!(config.db.password.as_deref(), Some("test_env_password_123"));
    assert_eq!(config.db.username.as_deref(), Some("test_env_user"));
    assert_eq!(config.db.endpoint, "ws://127.0.0.1:8000");
    assert_eq!(config.db.namespace, "data");
    assert_eq!(config.db.database, "ai");

    unsafe {
        std::env::remove_var("SURREAL_PASS");
        std::env::remove_var("SURREAL_USER");
        std::env::remove_var("SURREAL_URL");
        std::env::remove_var("SURREAL_NS");
        std::env::remove_var("SURREAL_DB");
        std::env::remove_var("GEMINI_API_KEY");
    }
}
