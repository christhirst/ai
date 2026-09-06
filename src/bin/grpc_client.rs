use ai::config::AppConfig;
use ai::grpc::{
    connect_client, ExecuteSurrealQlRequest, GetTableInfoRequest, ListTablesRequest,
    PopulateTableIntervalRequest, PopulateTableRequest,
};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "grpc_client")]
#[command(about = "CLI client for Tonic gRPC Table Populator & SurrealDB Agent")]
struct Cli {
    /// gRPC server address
    #[arg(short, long, default_value = "http://127.0.0.1:50051")]
    addr: String,

    /// Target SurrealDB table name
    #[arg(short, long)]
    table: Option<String>,

    /// Prompt for structured extraction
    #[arg(short, long)]
    prompt: Option<String>,

    /// Optional SurrealQL DDL statements (e.g. "DEFINE TABLE ... SCHEMAFULL; DEFINE FIELD ...")
    #[arg(short = 'd', long)]
    ddl: Option<String>,

    /// Optional namespace override
    #[arg(short = 'n', long)]
    namespace: Option<String>,

    /// Optional database override
    #[arg(long)]
    database: Option<String>,

    /// List all defined tables in SurrealDB
    #[arg(short = 'l', long)]
    list_tables: bool,

    /// Get schema and prompt comment for a specific table
    #[arg(short = 'i', long)]
    info: Option<String>,

    /// Optional Gemini model override
    #[arg(short = 'm', long)]
    model: Option<String>,

    /// Optional fields to omit from model schema and DB payload (e.g. -O status -O crime_id)
    #[arg(short = 'O', long = "omit-field")]
    omit_fields: Vec<String>,

    /// Optional interval step for time iteration: "daily", "weekly", "monthly", "yearly"
    #[arg(short = 'I', long)]
    interval: Option<String>,

    /// Start date for interval stepping (e.g. "2000-01" or "2000-01-01")
    #[arg(long)]
    start_date: Option<String>,

    /// End date for interval stepping (e.g. "2000-12" or "2000-12-31")
    #[arg(long)]
    end_date: Option<String>,

    /// Optional thinking level: "minimal", "low", "medium", "high"
    #[arg(long = "thinking-level")]
    thinking_level: Option<String>,

    /// Execute arbitrary SurrealQL query
    #[arg(short = 'q', long)]
    query: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let config = AppConfig::load().unwrap_or_else(|_| AppConfig {
        gemini_api_key: "".to_string(),
        model: "gemini-3.7-flash".to_string(),
        temperature: Some(0.0),
        preamble: None,
        variant: ai::config::ExecutionVariant::Typed,
        prompt: Default::default(),
        prompt_typed: Default::default(),
        db: Default::default(),
        grpc: Default::default(),
    });

    println!("Connecting to gRPC server at {}...", cli.addr);
    let mut client = connect_client(&cli.addr).await?;
    println!("Connected successfully.\n");

    if cli.list_tables {
        println!("=== Listing Defined Tables ===");
        let resp = client
            .list_tables(ListTablesRequest {
                namespace: cli.namespace.clone(),
                database: cli.database.clone(),
            })
            .await?
            .into_inner();
        println!("Tables ({}): {:?}", resp.tables.len(), resp.tables);
        return Ok(());
    }

    if let Some(tbl) = &cli.info {
        println!("=== Table Schema & Provenance Info for '{tbl}' ===");
        let resp = client
            .get_table_info(GetTableInfoRequest {
                table_name: tbl.clone(),
                namespace: cli.namespace.clone(),
                database: cli.database.clone(),
            })
            .await?
            .into_inner();
        println!("Table Name: {}", resp.table_name);
        println!("Prompt Comment: {}", if resp.comment.is_empty() { "(none)" } else { &resp.comment });
        println!("Fields: {:?}", resp.fields);
        println!("\nRaw Schema:\n{}", resp.schema_json);
        return Ok(());
    }

    if let Some(sql) = &cli.query {
        println!("=== Executing SurrealQL Query ===");
        println!("SQL: {sql}");
        let resp = client
            .execute_surreal_ql(ExecuteSurrealQlRequest {
                sql: sql.clone(),
                namespace: cli.namespace.clone(),
                database: cli.database.clone(),
            })
            .await?
            .into_inner();
        println!("Success: {}", resp.success);
        println!("Message: {}", resp.message);
        println!("Result:\n{}", resp.result_json);
        return Ok(());
    }

    // Default operation: PopulateTable
    let table = cli
        .table
        .unwrap_or_else(|| "homecides".to_string());
    let prompt = cli
        .prompt
        .unwrap_or_else(|| config.prompt_typed.query.clone());

    let model = cli.model.as_deref().unwrap_or(&config.model);

    let target_ns = cli.namespace.as_deref().unwrap_or(&config.db.namespace);
    let target_db = cli.database.as_deref().unwrap_or(&config.db.database);

    if let Some(interval) = cli.interval {
        let start_date = cli.start_date.expect("--start-date is required when --interval is specified");
        let end_date = cli.end_date.expect("--end-date is required when --interval is specified");

        println!("=== Populating SurrealDB Table iteratively over Intervals ===");
        println!("Target Namespace: {}", target_ns);
        println!("Target Database: {}", target_db);
        println!("Target Table: {}", table);
        println!("Interval: {}", interval);
        println!("Date Range: {} to {}", start_date, end_date);
        println!("Model: {}", model);
        if !cli.omit_fields.is_empty() {
            println!("Omit Fields: {:?}", cli.omit_fields);
        }
        if let Some(ddl) = &cli.ddl {
            println!("Provided DDL: {}", ddl);
        }

        let resp = client
            .populate_table_interval(PopulateTableIntervalRequest {
                prompt,
                table_name: table,
                interval,
                start_date,
                end_date,
                namespace: cli.namespace,
                database: cli.database,
                define_table_sql: cli.ddl,
                model: cli.model,
                temperature: None,
                preamble: None,
                enable_grounding: Some(true),
                omit_fields: cli.omit_fields.clone(),
                thinking_level: cli.thinking_level.clone(),
            })
            .await?
            .into_inner();

        println!("\n=== Interval Iterations Completed ===");
        println!("Success: {}", resp.success);
        println!("Message: {}", resp.message);
        println!("Completed Iterations: {}/{}", resp.completed_iterations, resp.completed_iterations + resp.failed_iterations);
        println!("Total Records Inserted: {}", resp.total_records_count);

        println!("\n=== Breakdown by Iteration ===");
        for (i, iter) in resp.iterations.iter().enumerate() {
            println!(
                "[{}/{}] Timeframe: {} ({} to {}) -> success={}, records={}, msg={}",
                i + 1,
                resp.iterations.len(),
                iter.timeframe,
                iter.start_date,
                iter.end_date,
                iter.success,
                iter.records_count,
                iter.message
            );
        }

        return Ok(());
    }

    println!("=== Populating SurrealDB Table via gRPC ===");
    println!("Target Namespace: {}", target_ns);
    println!("Target Database: {}", target_db);
    println!("Target Table: {}", table);
    println!("Model: {}", model);
    if !cli.omit_fields.is_empty() {
        println!("Omit Fields: {:?}", cli.omit_fields);
    }
    if let Some(tl) = &cli.thinking_level {
        println!("Thinking Level: {}", tl);
    }
    if let Some(ddl) = &cli.ddl {
        println!("Provided DDL: {}", ddl);
    }

    let resp = client
        .populate_table(PopulateTableRequest {
            prompt,
            table_name: table,
            namespace: cli.namespace,
            database: cli.database,
            define_table_sql: cli.ddl,
            model: cli.model,
            temperature: None,
            preamble: None,
            enable_grounding: Some(true),
            omit_fields: cli.omit_fields,
            thinking_level: cli.thinking_level,
        })
        .await?
        .into_inner();

    println!("\n=== Response ===");
    println!("Success: {}", resp.success);
    println!("Message: {}", resp.message);
    println!("Records Inserted: {}", resp.records_count);
    println!("Saved Table Comment: {}", resp.table_comment);
    println!("\nInserted Records Data:\n{}", resp.data_json);

    Ok(())
}
