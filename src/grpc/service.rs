use crate::config::{AppConfig, DatabaseConfig};
use crate::db::{
    execute_surrealql, get_defined_tables, get_table_schema_summary, init_db_from_config,
    insert_dynamic_records, set_table_comment, AppDb,
};
use crate::grpc::extractor::extract_table_data;
use crate::grpc::intervals::{
    generate_interval_steps, inject_timeframe_into_prompt, parse_interval, DateIntervalStep,
};
use crate::grpc::pb::table_populator_service_server::TablePopulatorService;
use crate::grpc::pb::{
    ExecuteSurrealQlRequest, ExecuteSurrealQlResponse, GetTableInfoRequest, GetTableInfoResponse,
    IntervalIterationResult, ListTablesRequest, ListTablesResponse, PopulateTableIntervalRequest,
    PopulateTableIntervalResponse, PopulateTableRequest, PopulateTableResponse,
};
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
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

    /// Prepares database, interval date steps, and filtered schema for an interval run.
    async fn prepare_interval_run(
        &self,
        prompt: &str,
        table_name: &str,
        interval_str: &str,
        start_date_str: &str,
        end_date_str: &str,
        ns_override: Option<&str>,
        db_override: Option<&str>,
        ddl_opt: Option<&str>,
        omit_fields: &[String],
    ) -> Result<(AppDb, Vec<DateIntervalStep>, Vec<String>, Vec<String>, String, String), Status> {
        if prompt.is_empty() {
            return Err(Status::invalid_argument("Prompt template must not be empty"));
        }
        if table_name.is_empty() {
            return Err(Status::invalid_argument("Table name must not be empty"));
        }
        if start_date_str.is_empty() {
            return Err(Status::invalid_argument("start_date must not be empty"));
        }
        if end_date_str.is_empty() {
            return Err(Status::invalid_argument("end_date must not be empty"));
        }

        let interval_type = parse_interval(interval_str)
            .map_err(|e| Status::invalid_argument(e))?;

        let steps = generate_interval_steps(interval_type, start_date_str, end_date_str)
            .map_err(|e| Status::invalid_argument(e))?;

        let db = self.get_db_for_request(ns_override, db_override).await?;

        // 1. If DDL statement is provided, execute once before intervals
        if let Some(ddl) = ddl_opt {
            let trimmed_ddl = ddl.trim();
            if !trimmed_ddl.is_empty() {
                tracing::info!(table = %table_name, ddl = %trimmed_ddl, "Executing provided DDL in SurrealDB before intervals");
                if let Err(e) = execute_surrealql(&db, trimmed_ddl).await {
                    tracing::error!(table = %table_name, error = %e, "Failed to execute DDL in SurrealDB");
                    return Err(Status::invalid_argument(format!("Failed to execute DDL in SurrealDB: {e}")));
                }
            }
        }

        // 2. Set table comment reflecting base prompt and date interval
        let comment_str = format!("{prompt} (Interval: {interval_str}, {start_date_str} to {end_date_str})");
        if let Err(e) = set_table_comment(&db, table_name, &comment_str).await {
            tracing::warn!(table = %table_name, error = %e, "Failed to set table comment");
        }

        // 3. Introspect schema
        let schema_summary = match get_table_schema_summary(&db, table_name).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(table = %table_name, error = %e, "Could not introspect table schema");
                crate::db::TableSchemaSummary {
                    table_name: table_name.to_string(),
                    comment: comment_str,
                    fields: Vec::new(),
                    field_names: Vec::new(),
                    raw_info: serde_json::Value::Null,
                }
            }
        };

        let omit_set: std::collections::HashSet<&str> = omit_fields
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let model_fields: Vec<String> = if omit_set.is_empty() {
            schema_summary.fields
        } else {
            schema_summary
                .fields
                .into_iter()
                .filter(|f| {
                    let (name, _, _) = crate::grpc::extractor::parse_field_info(f);
                    !omit_set.contains(name.as_str())
                })
                .collect()
        };

        let target_field_names: Vec<String> = if omit_set.is_empty() {
            schema_summary.field_names
        } else {
            schema_summary
                .field_names
                .into_iter()
                .filter(|name| !omit_set.contains(name.as_str()))
                .collect()
        };

        let effective_ns = ns_override.unwrap_or(&self.config.db.namespace).to_string();
        let effective_db = db_override.unwrap_or(&self.config.db.database).to_string();

        Ok((db, steps, model_fields, target_field_names, effective_ns, effective_db))
    }

    /// Executes extraction and insertion for a single interval step.
    async fn execute_single_interval_step(
        &self,
        step: &DateIntervalStep,
        template_prompt: &str,
        table_name: &str,
        db: &AppDb,
        model_fields: &[String],
        target_field_names: &[String],
        model: Option<&str>,
        temperature: Option<f64>,
        preamble: Option<&str>,
        enable_grounding: bool,
    ) -> IntervalIterationResult {
        let prompt = inject_timeframe_into_prompt(template_prompt, &step.timeframe_label);
        tracing::info!(
            table = %table_name,
            timeframe = %step.timeframe_label,
            "PopulateTableInterval starting step"
        );

        let records = match extract_table_data(
            &self.config,
            &prompt,
            table_name,
            model_fields,
            model,
            temperature,
            preamble,
            enable_grounding,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(
                    table = %table_name,
                    timeframe = %step.timeframe_label,
                    error = %e,
                    "PopulateTableInterval step extraction failed"
                );
                return IntervalIterationResult {
                    timeframe: step.timeframe_label.clone(),
                    start_date: step.start_date.clone(),
                    end_date: step.end_date.clone(),
                    success: false,
                    records_count: 0,
                    message: format!("Extraction failed: {e}"),
                    data_json: "[]".to_string(),
                };
            }
        };

        let sanitized_records = sanitize_and_map_records(records, target_field_names);

        let inserted = match insert_dynamic_records(db, table_name, &sanitized_records).await {
            Ok(ins) => ins,
            Err(e) => {
                tracing::error!(
                    table = %table_name,
                    timeframe = %step.timeframe_label,
                    error = %e,
                    "PopulateTableInterval step insertion failed"
                );
                return IntervalIterationResult {
                    timeframe: step.timeframe_label.clone(),
                    start_date: step.start_date.clone(),
                    end_date: step.end_date.clone(),
                    success: false,
                    records_count: 0,
                    message: format!("Insertion failed: {e}"),
                    data_json: "[]".to_string(),
                };
            }
        };

        let records_count = inserted.len() as u64;
        let data_json = serde_json::to_string_pretty(&inserted).unwrap_or_else(|_| "[]".to_string());

        IntervalIterationResult {
            timeframe: step.timeframe_label.clone(),
            start_date: step.start_date.clone(),
            end_date: step.end_date.clone(),
            success: true,
            records_count,
            message: format!(
                "Successfully populated table '{table_name}' with {records_count} records for timeframe '{}'.",
                step.timeframe_label
            ),
            data_json,
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

        let effective_model = req.model.as_deref().unwrap_or(&self.config.model);
        tracing::info!(
            table = %table_name,
            model = %effective_model,
            namespace = ?req.namespace,
            database = ?req.database,
            "Received request on server: PopulateTable"
        );

        if prompt.is_empty() {
            tracing::warn!("PopulateTable request rejected: prompt must not be empty");
            return Err(Status::invalid_argument("Prompt must not be empty"));
        }
        if table_name.is_empty() {
            tracing::warn!("PopulateTable request rejected: table name must not be empty");
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
                    tracing::error!(table = %table_name, error = %e, "Failed to execute DDL in SurrealDB");
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
                    field_names: Vec::new(),
                    raw_info: serde_json::Value::Null,
                }
            }
        };

        // Filter out any fields specified in req.omit_fields from model instructions and DB payload
        let omit_set: std::collections::HashSet<&str> = req
            .omit_fields
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if !omit_set.is_empty() {
            tracing::info!(
                table = %table_name,
                omitted_fields = ?req.omit_fields,
                "Omitting requested fields from LLM schema prompt and insertion payload"
            );
        }

        let model_fields: Vec<String> = if omit_set.is_empty() {
            schema_summary.fields.clone()
        } else {
            schema_summary
                .fields
                .iter()
                .filter(|f| {
                    let (name, _, _) = crate::grpc::extractor::parse_field_info(f);
                    !omit_set.contains(name.as_str())
                })
                .cloned()
                .collect()
        };

        let target_field_names: Vec<String> = if omit_set.is_empty() {
            schema_summary.field_names.clone()
        } else {
            schema_summary
                .field_names
                .iter()
                .filter(|name| !omit_set.contains(name.as_str()))
                .cloned()
                .collect()
        };

        // 4. Extract structured data using Rig / Gemini
        let enable_grounding = req.enable_grounding.unwrap_or(true);
        tracing::info!(
            table = %table_name,
            model = %effective_model,
            "PopulateTable-Agent starting data extraction"
        );
        let records = match extract_table_data(
            &self.config,
            &prompt,
            &table_name,
            &model_fields,
            req.model.as_deref(),
            req.temperature,
            req.preamble.as_deref(),
            enable_grounding,
        )
        .await
        {
            Ok(r) => {
                tracing::info!(
                    table = %table_name,
                    records_count = r.len(),
                    "PopulateTable-Agent completed data extraction"
                );
                r
            }
            Err(e) => {
                tracing::error!(
                    table = %table_name,
                    error = %e,
                    "PopulateTable-Agent data extraction failed"
                );
                return Err(Status::internal(format!("Agent extraction failed: {e}")));
            }
        };

        // 5. Sanitize records to strictly conform to target schema and prune omitted fields
        let sanitized_records = sanitize_and_map_records(records, &target_field_names);

        // 6. Insert structured records into SurrealDB
        tracing::info!(
            table = %table_name,
            records_count = sanitized_records.len(),
            "PopulateTable-Agent inserting records into SurrealDB"
        );
        let inserted = match insert_dynamic_records(&db, &table_name, &sanitized_records).await {
            Ok(ins) => {
                tracing::info!(
                    table = %table_name,
                    inserted_count = ins.len(),
                    "PopulateTable-Agent successfully inserted records into SurrealDB"
                );
                ins
            }
            Err(e) => {
                tracing::error!(
                    table = %table_name,
                    error = %e,
                    "PopulateTable-Agent failed to insert records into SurrealDB"
                );
                return Err(Status::internal(format!("Failed to insert records into SurrealDB table '{table_name}': {e}")));
            }
        };

        let records_count = inserted.len() as u64;
        let data_json = serde_json::to_string_pretty(&inserted).unwrap_or_else(|_| "[]".to_string());
        let table_schema_json = serde_json::to_string_pretty(&schema_summary.raw_info)
            .unwrap_or_else(|_| "{}".to_string());

        let effective_ns = req.namespace.as_deref().unwrap_or(&self.config.db.namespace);
        let effective_db = req.database.as_deref().unwrap_or(&self.config.db.database);

        let reply = PopulateTableResponse {
            success: true,
            table_name: table_name.clone(),
            records_count,
            data_json,
            table_comment: schema_summary.comment,
            table_schema_json,
            message: format!(
                "Successfully populated table '{table_name}' in namespace '{effective_ns}', database '{effective_db}' with {records_count} records."
            ),
        };

        tracing::info!(
            table = %table_name,
            records_count,
            "PopulateTable-Agent completed successfully"
        );

        Ok(Response::new(reply))
    }

    async fn get_table_info(
        &self,
        request: Request<GetTableInfoRequest>,
    ) -> Result<Response<GetTableInfoResponse>, Status> {
        let req = request.into_inner();
        let table_name = req.table_name.trim();

        tracing::info!(
            table = %table_name,
            namespace = ?req.namespace,
            database = ?req.database,
            "Received request on server: GetTableInfo"
        );

        if table_name.is_empty() {
            tracing::warn!("GetTableInfo request rejected: table name must not be empty");
            return Err(Status::invalid_argument("Table name must not be empty"));
        }

        let db = self
            .get_db_for_request(req.namespace.as_deref(), req.database.as_deref())
            .await?;

        let summary = get_table_schema_summary(&db, table_name)
            .await
            .map_err(|e| {
                tracing::error!(table = %table_name, error = %e, "Failed to query table info");
                Status::internal(format!("Failed to query table info for '{table_name}': {e}"))
            })?;

        let schema_json = serde_json::to_string_pretty(&summary.raw_info).unwrap_or_default();

        let reply = GetTableInfoResponse {
            table_name: summary.table_name,
            comment: summary.comment,
            schema_json,
            fields: summary.fields,
        };

        tracing::info!(table = %table_name, "GetTableInfo request completed successfully");
        Ok(Response::new(reply))
    }

    async fn list_tables(
        &self,
        request: Request<ListTablesRequest>,
    ) -> Result<Response<ListTablesResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(
            namespace = ?req.namespace,
            database = ?req.database,
            "Received request on server: ListTables"
        );

        let db = self
            .get_db_for_request(req.namespace.as_deref(), req.database.as_deref())
            .await?;

        let tables = get_defined_tables(&db)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to list tables");
                Status::internal(format!("Failed to list tables: {e}"))
            })?;

        tracing::info!(tables_count = tables.len(), "ListTables request completed successfully");
        Ok(Response::new(ListTablesResponse { tables }))
    }

    async fn execute_surreal_ql(
        &self,
        request: Request<ExecuteSurrealQlRequest>,
    ) -> Result<Response<ExecuteSurrealQlResponse>, Status> {
        let req = request.into_inner();
        let sql = req.sql.trim();

        tracing::info!(
            sql = %sql,
            namespace = ?req.namespace,
            database = ?req.database,
            "Received request on server: ExecuteSurrealQl"
        );

        if sql.is_empty() {
            tracing::warn!("ExecuteSurrealQl request rejected: SQL query must not be empty");
            return Err(Status::invalid_argument("SQL query must not be empty"));
        }

        let db = self
            .get_db_for_request(req.namespace.as_deref(), req.database.as_deref())
            .await?;

        match execute_surrealql(&db, sql).await {
            Ok(result) => {
                let result_json = serde_json::to_string_pretty(&result).unwrap_or_default();
                tracing::info!("ExecuteSurrealQl request completed successfully");
                Ok(Response::new(ExecuteSurrealQlResponse {
                    success: true,
                    result_json,
                    message: "Query executed successfully".to_string(),
                }))
            }
            Err(e) => {
                tracing::error!(error = %e, "ExecuteSurrealQl query failed");
                Ok(Response::new(ExecuteSurrealQlResponse {
                    success: false,
                    result_json: "{}".to_string(),
                    message: format!("Query failed: {e}"),
                }))
            }
        }
    }

    async fn populate_table_interval(
        &self,
        request: Request<PopulateTableIntervalRequest>,
    ) -> Result<Response<PopulateTableIntervalResponse>, Status> {
        let req = request.into_inner();
        let prompt = req.prompt.trim().to_string();
        let table_name = req.table_name.trim().to_string();

        let (db, steps, model_fields, target_field_names, effective_ns, effective_db) = self
            .prepare_interval_run(
                &prompt,
                &table_name,
                &req.interval,
                &req.start_date,
                &req.end_date,
                req.namespace.as_deref(),
                req.database.as_deref(),
                req.define_table_sql.as_deref(),
                &req.omit_fields,
            )
            .await?;

        let enable_grounding = req.enable_grounding.unwrap_or(true);
        let total_steps = steps.len();
        let mut iterations = Vec::with_capacity(total_steps);
        let mut total_records_count = 0u64;
        let mut completed_iterations = 0u32;
        let mut failed_iterations = 0u32;

        for step in steps {
            let res = self
                .execute_single_interval_step(
                    &step,
                    &prompt,
                    &table_name,
                    &db,
                    &model_fields,
                    &target_field_names,
                    req.model.as_deref(),
                    req.temperature,
                    req.preamble.as_deref(),
                    enable_grounding,
                )
                .await;

            if res.success {
                completed_iterations += 1;
                total_records_count += res.records_count;
            } else {
                failed_iterations += 1;
            }
            iterations.push(res);
        }

        let reply = PopulateTableIntervalResponse {
            success: failed_iterations == 0,
            table_name: table_name.clone(),
            total_records_count,
            completed_iterations,
            failed_iterations,
            iterations,
            message: format!(
                "PopulateTableInterval for table '{table_name}' in namespace '{effective_ns}', database '{effective_db}': {completed_iterations}/{total_steps} iterations successful, {total_records_count} records inserted."
            ),
        };

        Ok(Response::new(reply))
    }

    type PopulateTableIntervalStreamStream = ReceiverStream<Result<IntervalIterationResult, Status>>;

    async fn populate_table_interval_stream(
        &self,
        request: Request<PopulateTableIntervalRequest>,
    ) -> Result<Response<Self::PopulateTableIntervalStreamStream>, Status> {
        let req = request.into_inner();
        let prompt = req.prompt.trim().to_string();
        let table_name = req.table_name.trim().to_string();

        let (db, steps, model_fields, target_field_names, _ns, _db) = self
            .prepare_interval_run(
                &prompt,
                &table_name,
                &req.interval,
                &req.start_date,
                &req.end_date,
                req.namespace.as_deref(),
                req.database.as_deref(),
                req.define_table_sql.as_deref(),
                &req.omit_fields,
            )
            .await?;

        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let this = Arc::new(TablePopulatorServiceImpl {
            config: Arc::clone(&self.config),
            db: Arc::clone(&self.db),
        });

        tokio::spawn(async move {
            let enable_grounding = req.enable_grounding.unwrap_or(true);
            for step in steps {
                let res = this
                    .execute_single_interval_step(
                        &step,
                        &prompt,
                        &table_name,
                        &db,
                        &model_fields,
                        &target_field_names,
                        req.model.as_deref(),
                        req.temperature,
                        req.preamble.as_deref(),
                        enable_grounding,
                    )
                    .await;

                if tx.send(Ok(res)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// Maps known alias fields and filters out unpermitted fields according to the introspected table schema.
pub fn sanitize_and_map_records(
    records: Vec<serde_json::Value>,
    field_names: &[String],
) -> Vec<serde_json::Value> {
    if field_names.is_empty() {
        return records;
    }

    let allowed_keys: std::collections::HashSet<&str> = field_names
        .iter()
        .map(|s| s.as_str())
        .chain(std::iter::once("id"))
        .collect();

    records
        .into_iter()
        .map(|record| {
            if let serde_json::Value::Object(mut map) = record {
                // 1. Alias date / crime_date -> incident_date if allowed and missing
                if allowed_keys.contains("incident_date") && !map.contains_key("incident_date") {
                    if let Some(val) = map.remove("date").or_else(|| map.remove("crime_date")) {
                        map.insert("incident_date".to_string(), val);
                    }
                }

                // 2. Alias link / source_url -> url if allowed and missing
                if allowed_keys.contains("url") && !map.contains_key("url") {
                    if let Some(val) = map.remove("link").or_else(|| map.remove("source_url")) {
                        map.insert("url".to_string(), val);
                    }
                }

                // 3. Alias query -> discovery_query if allowed and missing
                if allowed_keys.contains("discovery_query") && !map.contains_key("discovery_query") {
                    if let Some(val) = map.remove("query") {
                        map.insert("discovery_query".to_string(), val);
                    }
                }

                // 4. Merge title / incident_title / headline into raw_text if raw_text is allowed but title is not
                if allowed_keys.contains("raw_text") {
                    let title_val = if !allowed_keys.contains("incident_title") {
                        map.remove("incident_title")
                    } else {
                        None
                    }
                    .or_else(|| {
                        if !allowed_keys.contains("title") {
                            map.remove("title")
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        if !allowed_keys.contains("headline") {
                            map.remove("headline")
                        } else {
                            None
                        }
                    });

                    if let Some(title) = title_val {
                        if let Some(title_str) = title.as_str() {
                            if let Some(existing_raw) = map.get_mut("raw_text") {
                                if let Some(raw_str) = existing_raw.as_str() {
                                    if !raw_str.contains(title_str) {
                                        *existing_raw =
                                            serde_json::json!(format!("{}: {}", title_str, raw_str));
                                    }
                                }
                            } else {
                                map.insert(
                                    "raw_text".to_string(),
                                    serde_json::Value::String(title_str.to_string()),
                                );
                            }
                        }
                    }
                }

                // 5. Filter only allowed keys
                let mut filtered = serde_json::Map::new();
                for (k, v) in map {
                    if allowed_keys.contains(k.as_str()) {
                        filtered.insert(k, v);
                    } else {
                        tracing::warn!(
                            dropped_field = %k,
                            "Pruned undeclared field from record to strictly conform to table schema"
                        );
                    }
                }
                serde_json::Value::Object(filtered)
            } else {
                record
            }
        })
        .collect()
}

