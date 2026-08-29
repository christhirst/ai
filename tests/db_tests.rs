use ai::config::{AppConfig, DatabaseConfig};
use ai::db::{
    AnyDb, LocalDb, create_gdp_record, create_gdp_records, create_homecide_record,
    create_homecide_records, delete_all_gdp_records, delete_all_homecides,
    delete_gdp_records_by_year, delete_homecides_by_source, get_all_gdp_records, get_all_homecides,
    get_gdp_records_by_year, get_homecides_by_min_amount, get_homecides_by_source,
    init_db_from_config, init_memory_db,
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

    let config_db: AnyDb = init_db_from_config(&config)
        .await
        .expect("Failed to init db from config");

    let gdp_initial: Vec<GdpRecord> = get_all_gdp_records(&db).await.unwrap();
    assert!(gdp_initial.is_empty());

    let gdp_custom_initial: Vec<GdpRecord> = get_all_gdp_records(&config_db).await.unwrap();
    assert!(gdp_custom_initial.is_empty());
}

#[tokio::test]
async fn test_app_config_db_loading() {
    if std::env::var("GEMINI_API_KEY").is_err() && std::env::var("APP_GEMINI_API_KEY").is_err() {
        unsafe {
            std::env::set_var("GEMINI_API_KEY", "test_key");
        }
    }
    let config = AppConfig::load().expect("Failed to load AppConfig");
    assert_eq!(config.db.endpoint, "mem://");
    assert_eq!(config.db.namespace, "ai");
    assert_eq!(config.db.database, "records");

    let db = init_db_from_config(&config.db)
        .await
        .expect("Failed to connect using loaded config");

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
        amount: 2500,
        source: "BKA Police Report".to_string(),
    };
    let created = create_homecide_record(&db, &record1)
        .await
        .expect("Failed to insert single record");
    assert!(created.is_some());
    let created_unwrapped = created.unwrap();
    assert_eq!(created_unwrapped.amount, 2500);
    assert_eq!(created_unwrapped.source, "BKA Police Report");

    // 2. Create bulk records
    let bulk_records = vec![
        Homecides {
            amount: 3100,
            source: "Eurostat".to_string(),
        },
        Homecides {
            amount: 1800,
            source: "Eurostat".to_string(),
        },
        Homecides {
            amount: 4500,
            source: "WHO Global Health".to_string(),
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

    // 4. Query by source
    let eurostat_records = get_homecides_by_source(&db, "Eurostat")
        .await
        .expect("Failed to get homecides by source");
    assert_eq!(eurostat_records.len(), 2);
    assert!(eurostat_records.iter().all(|h| h.source == "Eurostat"));

    // 5. Query by min amount
    let high_crime = get_homecides_by_min_amount(&db, 3000)
        .await
        .expect("Failed to filter by min amount");
    assert_eq!(high_crime.len(), 2);
    assert!(high_crime.iter().all(|h| h.amount >= 3000));

    // 6. Delete by source
    let deleted_eurostat = delete_homecides_by_source(&db, "Eurostat")
        .await
        .expect("Failed to delete by source");
    assert_eq!(deleted_eurostat.len(), 2);

    let remaining_eurostat = get_homecides_by_source(&db, "Eurostat")
        .await
        .expect("Failed to query eurostat after delete");
    assert!(remaining_eurostat.is_empty());

    let remaining_all = get_all_homecides(&db)
        .await
        .expect("Failed to get all records after deletion");
    assert_eq!(remaining_all.len(), 2);

    // 7. Delete all
    let deleted_all = delete_all_homecides(&db)
        .await
        .expect("Failed to delete all records");
    assert_eq!(deleted_all.len(), 2);

    let final_check = get_all_homecides(&db)
        .await
        .expect("Failed to fetch records after purge");
    assert!(final_check.is_empty());
}
