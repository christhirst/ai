use crate::config::{AppConfig, DatabaseConfig};
use crate::db::{
    execute_surrealql, get_defined_tables, get_table_schema_summary, init_db_from_config,
    insert_dynamic_records, set_table_comment, AppDb,
};
use crate::grpc::extractor::extract_table_data;
use crate::grpc::pb::table_populator_service_server::TablePopulatorService;
use crate::grpc::pb::{
    ExecuteSurrealQlRequest, ExecuteSurrealQlResponse, GetTableInfoRequest, GetTableInfoResponse,
    ListTablesRequest, ListTablesResponse, PopulateTableRequest, PopulateTableResponse,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};

pub struct TablePopulatorServiceImpl {
    pub config: Arc<AppConfig>,
    pub db: Arc<AppDb>,
}

impl TablePopulatorServiceImpl {
    pub fn new(config: Arc<AppConfig>, db: Arc<AppDb>) -> Self {
        Self { config, db }
    }

    /// Resolves the database instance to use based on optional request overrides for namespace/database.
    async fn get_db_for_request(
        &self,
        ns_override: Option<&str>,
        db_override: Option<&str>,
    ) -> Result<AppDb, Status> {
        let ns = ns_override.unwrap_or(&self.config.db.namespace);
        let db = db_override.unwrap_or(&self.config.db.database);

        if ns == self.config.db.namespace && db == self.config.db.database {
            Ok((*self.db).clone())
        } else {
            let custom_cfg = DatabaseConfig {
                endpoint: self.config.db.endpoint.clone(),
                namespace: ns.to_string(),
                database: db.to_string(),
                username: self.config.db.username.clone(),
                password: self.config.db.password.clone(),
            };
            init_db_from_config(&custom_cfg)
                .await
                .map_err(|e| Status::internal(format!("Failed to connect to target SurrealDB {ns}/{db}: {e}")))
        }
    }
}

#[tonic::async_trait]
impl TablePopulatorService for TablePopulatorServiceImpl {
    async fn populate_table(
        &self,
        request: Request<PopulateTableRequest>,
    ) -> Result<Response<PopulateTableResponse>, Status> {
        let req = request.into_inner();
        let prompt = req.prompt.trim().to_string();
        let table_name = req.table_name.trim().to_string();

        if prompt.is_empty() {
            return Err(Status::invalid_argument("Prompt must not be empty"));
        }
        if table_name.is_empty() {
            return Err(Status::invalid_argument("Table name must not be empty"));
        }

        let db = self
            .get_db_for_request(req.namespace.as_deref(), req.database.as_deref())
            .await?;

        // 1. If DDL statement is provided, execute it first
        if let Some(ddl) = &req.define_table_sql {
            let trimmed_ddl = ddl.trim();
            if !trimmed_ddl.is_empty() {
                tracing::info!(table = %table_name, ddl = %trimmed_ddl, "Executing provided DDL in SurrealDB");
                if let Err(e) = execute_surrealql(&db, trimmed_ddl).await {
                    return Err(Status::invalid_argument(format!("Failed to execute DDL in SurrealDB: {e}")));
                }
            }
        }

        // 2. Set the prompt as the table COMMENT
        if let Err(e) = set_table_comment(&db, &table_name, &prompt).await {
            tracing::warn!(table = %table_name, error = %e, "Failed to set table comment");
        }

        // 3. Introspect table schema to extract fields
        let schema_summary = match get_table_schema_summary(&db, &table_name).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(table = %table_name, error = %e, "Could not introspect table schema");
                crate::db::TableSchemaSummary {
                    table_name: table_name.clone(),
                    comment: prompt.clone(),
                    fields: Vec::new(),
                    raw_info: serde_json::Value::Null,
                }
            }
        };

        // 4. Extract structured data using Rig / Gemini
        let enable_grounding = req.enable_grounding.unwrap_or(true);
        let records = match extract_table_data(
            &self.config,
            &prompt,
            &table_name,
            &schema_summary.fields,
            req.model.as_deref(),
            req.temperature,
            req.preamble.as_deref(),
            enable_grounding,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err(Status::internal(format!("Agent extraction failed: {e}")));
            }
        };

        // 5. Insert structured records into SurrealDB
        let inserted = match insert_dynamic_records(&db, &table_name, &records).await {
            Ok(ins) => ins,
            Err(e) => {
                return Err(Status::internal(format!("Failed to insert records into SurrealDB table '{table_name}': {e}")));
            }
        };

        let records_count = inserted.len() as u64;
        let data_json = serde_json::to_string_pretty(&inserted).unwrap_or_else(|_| "[]".to_string());
        let table_schema_json = serde_json::to_string_pretty(&schema_summary.raw_info)
            .unwrap_or_else(|_| "{}".to_string());

        let reply = PopulateTableResponse {
            success: true,
            table_name: table_name.clone(),
            records_count,
            data_json,
            table_comment: schema_summary.comment,
            table_schema_json,
            message: format!("Successfully populated table '{table_name}' with {records_count} records."),
        };

        Ok(Response::new(reply))
    }

    async fn get_table_info(
        &self,
        request: Request<GetTableInfoRequest>,
    ) -> Result<Response<GetTableInfoResponse>, Status> {
        let req = request.into_inner();
        let table_name = req.table_name.trim();
        if table_name.is_empty() {
            return Err(Status::invalid_argument("Table name must not be empty"));
        }

        let db = self
            .get_db_for_request(req.namespace.as_deref(), req.database.as_deref())
            .await?;

        let summary = get_table_schema_summary(&db, table_name)
            .await
            .map_err(|e| Status::internal(format!("Failed to query table info for '{table_name}': {e}")))?;

        let schema_json = serde_json::to_string_pretty(&summary.raw_info).unwrap_or_default();

        let reply = GetTableInfoResponse {
            table_name: summary.table_name,
            comment: summary.comment,
            schema_json,
            fields: summary.fields,
        };

        Ok(Response::new(reply))
    }

    async fn list_tables(
        &self,
        request: Request<ListTablesRequest>,
    ) -> Result<Response<ListTablesResponse>, Status> {
        let req = request.into_inner();
        let db = self
            .get_db_for_request(req.namespace.as_deref(), req.database.as_deref())
            .await?;

        let tables = get_defined_tables(&db)
            .await
            .map_err(|e| Status::internal(format!("Failed to list tables: {e}")))?;

        Ok(Response::new(ListTablesResponse { tables }))
    }

    async fn execute_surreal_ql(
        &self,
        request: Request<ExecuteSurrealQlRequest>,
    ) -> Result<Response<ExecuteSurrealQlResponse>, Status> {
        let req = request.into_inner();
        let sql = req.sql.trim();
        if sql.is_empty() {
            return Err(Status::invalid_argument("SQL query must not be empty"));
        }

        let db = self
            .get_db_for_request(req.namespace.as_deref(), req.database.as_deref())
            .await?;

        match execute_surrealql(&db, sql).await {
            Ok(result) => {
                let result_json = serde_json::to_string_pretty(&result).unwrap_or_default();
                Ok(Response::new(ExecuteSurrealQlResponse {
                    success: true,
                    result_json,
                    message: "Query executed successfully".to_string(),
                }))
            }
            Err(e) => Ok(Response::new(ExecuteSurrealQlResponse {
                success: false,
                result_json: "{}".to_string(),
                message: format!("Query failed: {e}"),
            })),
        }
    }
}
