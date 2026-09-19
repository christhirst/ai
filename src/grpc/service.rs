use crate::config::{AppConfig, DatabaseConfig};
use crate::db::{
    AppDb, execute_surrealql, get_defined_tables, get_table_schema_summary, init_db_from_config,
    insert_dynamic_records, set_table_comment,
};
use crate::grpc::extractor::{extract_table_data, fact_check_record};
use crate::grpc::intervals::{
    DateIntervalStep, generate_interval_steps, inject_timeframe_into_prompt, parse_flexible_date,
    parse_interval,
};
use crate::grpc::pb::table_populator_service_server::TablePopulatorService;
use crate::grpc::pb::{
    ExecuteSurrealQlRequest, ExecuteSurrealQlResponse, FactCheckEntriesRequest,
    FactCheckEntriesResponse, FactCheckResult, GetTableInfoRequest, GetTableInfoResponse,
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
            init_db_from_config(&custom_cfg).await.map_err(|e| {
                Status::internal(format!(
                    "Failed to connect to target SurrealDB {ns}/{db}: {e}"
                ))
            })
        }
    }

    /// Prepares database, interval date steps, and filtered schema for an interval run.
    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<
        (
            AppDb,
            Vec<DateIntervalStep>,
            Vec<String>,
            Vec<String>,
            String,
            String,
        ),
        Status,
    > {
        if prompt.is_empty() {
            return Err(Status::invalid_argument(
                "Prompt template must not be empty",
            ));
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

        let interval_type = parse_interval(interval_str).map_err(Status::invalid_argument)?;

        let steps = generate_interval_steps(interval_type, start_date_str, end_date_str)
            .map_err(Status::invalid_argument)?;

        let db = self.get_db_for_request(ns_override, db_override).await?;

        // 1. If DDL statement is provided, execute once before intervals
        if let Some(ddl) = ddl_opt {
            let trimmed_ddl = ddl.trim();
            if !trimmed_ddl.is_empty() {
                tracing::info!(table = %table_name, ddl = %trimmed_ddl, "Executing provided DDL in SurrealDB before intervals");
                if let Err(e) = execute_surrealql(&db, trimmed_ddl).await {
                    tracing::error!(table = %table_name, error = %e, "Failed to execute DDL in SurrealDB");
                    return Err(Status::invalid_argument(format!(
                        "Failed to execute DDL in SurrealDB: {e}"
                    )));
                }
            }
        }

        // 2. Set table comment reflecting base prompt and date interval
        let comment_str =
            format!("{prompt} (Interval: {interval_str}, {start_date_str} to {end_date_str})");
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

        Ok((
            db,
            steps,
            model_fields,
            target_field_names,
            effective_ns,
            effective_db,
        ))
    }

    /// Executes extraction and insertion for a single interval step.
    #[allow(clippy::too_many_arguments)]
    async fn execute_single_interval_step(
        &self,
        step: &DateIntervalStep,
        template_prompt: &str,
        table_name: &str,
        db: &AppDb,
        model_fields: &[String],
        target_field_names: &[String],
        provider: Option<&str>,
        model: Option<&str>,
        temperature: Option<f64>,
        preamble: Option<&str>,
        enable_grounding: bool,
        thinking_level: Option<&str>,
        base_url: Option<&str>,
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
            provider,
            model,
            temperature,
            preamble,
            enable_grounding,
            thinking_level,
            base_url,
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
        let data_json =
            serde_json::to_string_pretty(&inserted).unwrap_or_else(|_| "[]".to_string());

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

    /// Builds the query to fetch records for fact-checking and returns (AppDb, Vec<serde_json::Value>).
    async fn fetch_records_for_fact_check(
        &self,
        req: &FactCheckEntriesRequest,
    ) -> Result<(AppDb, Vec<serde_json::Value>), Status> {
        let table_name = req.table_name.trim();
        if table_name.is_empty() {
            return Err(Status::invalid_argument("Table name must not be empty"));
        }

        let has_ids = !req.record_ids.is_empty();
        let has_date = req
            .start_date
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
            || req
                .end_date
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
        let has_status = req
            .status_filter
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

        if !has_ids && !has_date && !has_status {
            return Err(Status::invalid_argument(
                "At least one filter must be provided: 'record_ids', 'start_date'/'end_date', or 'status_filter'.",
            ));
        }

        let db = self
            .get_db_for_request(req.namespace.as_deref(), req.database.as_deref())
            .await?;

        let mut where_clauses = Vec::new();

        // 1. Record IDs filter
        if has_ids {
            let normalized_ids: Vec<String> = req
                .record_ids
                .iter()
                .map(|id| {
                    let trimmed = id.trim();
                    if trimmed.contains(':') {
                        trimmed.to_string()
                    } else {
                        format!("{table_name}:{trimmed}")
                    }
                })
                .collect();
            where_clauses.push(format!("id IN [{}]", normalized_ids.join(", ")));
        }

        // 2. Date range filter
        if has_date {
            let date_field = req
                .date_field
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("incident_date");

            let start_opt = if let Some(ref s) = req.start_date {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    Some(parse_flexible_date(trimmed, false).map_err(Status::invalid_argument)?)
                } else {
                    None
                }
            } else {
                None
            };

            let end_opt = if let Some(ref e) = req.end_date {
                let trimmed = e.trim();
                if !trimmed.is_empty() {
                    Some(parse_flexible_date(trimmed, true).map_err(Status::invalid_argument)?)
                } else {
                    None
                }
            } else {
                None
            };

            match (start_opt, end_opt) {
                (Some(start_d), Some(end_d)) => {
                    let start_str = start_d.format("%Y-%m-%d").to_string();
                    let end_str = end_d.format("%Y-%m-%d").to_string();
                    where_clauses.push(format!(
                        "(({date_field} >= <datetime>'{start_str}T00:00:00Z' AND {date_field} <= <datetime>'{end_str}T23:59:59Z') OR ({date_field} >= '{start_str}' AND {date_field} <= '{end_str}'))"
                    ));
                }
                (Some(start_d), None) => {
                    let start_str = start_d.format("%Y-%m-%d").to_string();
                    where_clauses.push(format!(
                        "({date_field} >= <datetime>'{start_str}T00:00:00Z' OR {date_field} >= '{start_str}')"
                    ));
                }
                (None, Some(end_d)) => {
                    let end_str = end_d.format("%Y-%m-%d").to_string();
                    where_clauses.push(format!(
                        "({date_field} <= <datetime>'{end_str}T23:59:59Z' OR {date_field} <= '{end_str}')"
                    ));
                }
                (None, None) => {}
            }
        }

        // 3. Status filter
        if let Some(ref st) = req.status_filter {
            let trimmed = st.trim();
            if !trimmed.is_empty() {
                let escaped = trimmed.replace('\'', "\\'");
                where_clauses.push(format!("status = '{escaped}'"));
            }
        }

        let mut query = format!("SELECT * FROM {table_name}");
        if !where_clauses.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&where_clauses.join(" AND "));
        }

        if let Some(lim) = req.limit {
            if lim > 0 {
                query.push_str(&format!(" LIMIT {lim}"));
            }
        }
        query.push(';');

        tracing::info!(table = %table_name, sql = %query, "Executing query to fetch records for fact-checking");

        let raw_val = execute_surrealql(&db, &query).await.map_err(|e| {
            tracing::error!(table = %table_name, error = %e, "Failed to query records for fact-checking");
            Status::internal(format!("Failed to query records for fact-checking: {e}"))
        })?;

        let records: Vec<serde_json::Value> = match raw_val {
            serde_json::Value::Array(arr) => {
                if let Some(first) = arr.first()
                    && let Some(inner_arr) = first.get("result").and_then(|r| r.as_array())
                {
                    inner_arr.clone()
                } else {
                    arr
                }
            }
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Array(arr)) = map.get("result") {
                    arr.clone()
                } else {
                    vec![serde_json::Value::Object(map)]
                }
            }
            _ => Vec::new(),
        };

        Ok((db, records))
    }

    /// Executes a fact-check on a single record and optionally updates its status in SurrealDB.
    #[allow(clippy::too_many_arguments)]
    async fn execute_single_fact_check(
        &self,
        db: &AppDb,
        table_name: &str,
        record: &serde_json::Value,
        update_db_status: bool,
        provider: Option<&str>,
        model: Option<&str>,
        temperature: Option<f64>,
        preamble: Option<&str>,
        thinking_level: Option<&str>,
        base_url: Option<&str>,
    ) -> FactCheckResult {
        let record_id = match record.get("id") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Object(o)) => {
                if let (Some(tb), Some(id)) = (o.get("tb"), o.get("id")) {
                    format!("{}:{}", tb.as_str().unwrap_or_default(), id.as_str().unwrap_or_default())
                } else {
                    serde_json::to_string(o).unwrap_or_default()
                }
            }
            Some(other) => other.to_string().trim_matches('"').to_string(),
            None => String::new(),
        };

        let record_json = serde_json::to_string_pretty(record).unwrap_or_else(|_| record.to_string());

        let outcome = match fact_check_record(
            &self.config,
            &record_json,
            table_name,
            provider,
            model,
            temperature,
            preamble,
            thinking_level,
            base_url,
        )
        .await
        {
            Ok(out) => out,
            Err(e) => {
                tracing::error!(
                    table = %table_name,
                    record_id = %record_id,
                    error = %e,
                    "Fact-check LLM call failed"
                );
                return FactCheckResult {
                    record_id,
                    record_json,
                    verdict: "unverifiable".to_string(),
                    status: "Discarded_Irrelevant".to_string(),
                    explanation: format!("Fact check failed: {e}"),
                    sources: Vec::new(),
                    success: false,
                    message: format!("LLM verification error: {e}"),
                    db_updated: false,
                };
            }
        };

        let mut db_updated = false;
        if update_db_status && !record_id.is_empty() {
            let full_id = if record_id.contains(':') {
                record_id.clone()
            } else {
                format!("{table_name}:{record_id}")
            };
            let update_sql = format!("UPDATE {full_id} SET status = '{}';", outcome.status);
            match execute_surrealql(db, &update_sql).await {
                Ok(_) => {
                    tracing::info!(
                        table = %table_name,
                        record_id = %full_id,
                        status = %outcome.status,
                        "Record status updated in SurrealDB"
                    );
                    db_updated = true;
                }
                Err(e) => {
                    tracing::warn!(
                        table = %table_name,
                        record_id = %full_id,
                        error = %e,
                        "Failed to update record status in SurrealDB"
                    );
                }
            }
        }

        FactCheckResult {
            record_id,
            record_json,
            verdict: outcome.verdict,
            status: outcome.status,
            explanation: outcome.explanation,
            sources: outcome.sources,
            success: true,
            message: "Fact check completed successfully.".to_string(),
            db_updated,
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
        let effective_provider = req
            .provider
            .as_deref()
            .unwrap_or(match self.config.provider {
                crate::config::ModelProvider::Gemini => "gemini",
                crate::config::ModelProvider::Qwen => "qwen",
            });
        tracing::info!(
            table = %table_name,
            provider = %effective_provider,
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
                    return Err(Status::invalid_argument(format!(
                        "Failed to execute DDL in SurrealDB: {e}"
                    )));
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

        // 4. Extract structured data using Rig / Gemini / Qwen
        let enable_grounding = req.enable_grounding.unwrap_or(true);
        tracing::info!(
            table = %table_name,
            provider = %effective_provider,
            model = %effective_model,
            "PopulateTable-Agent starting data extraction"
        );
        let records = match extract_table_data(
            &self.config,
            &prompt,
            &table_name,
            &model_fields,
            req.provider.as_deref(),
            req.model.as_deref(),
            req.temperature,
            req.preamble.as_deref(),
            enable_grounding,
            req.thinking_level.as_deref(),
            req.base_url.as_deref(),
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
                return Err(Status::internal(format!(
                    "Failed to insert records into SurrealDB table '{table_name}': {e}"
                )));
            }
        };

        let records_count = inserted.len() as u64;
        let data_json =
            serde_json::to_string_pretty(&inserted).unwrap_or_else(|_| "[]".to_string());
        let table_schema_json = serde_json::to_string_pretty(&schema_summary.raw_info)
            .unwrap_or_else(|_| "{}".to_string());

        let effective_ns = req
            .namespace
            .as_deref()
            .unwrap_or(&self.config.db.namespace);
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
                Status::internal(format!(
                    "Failed to query table info for '{table_name}': {e}"
                ))
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

        let tables = get_defined_tables(&db).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to list tables");
            Status::internal(format!("Failed to list tables: {e}"))
        })?;

        tracing::info!(
            tables_count = tables.len(),
            "ListTables request completed successfully"
        );
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

        let thinking_level = req.thinking_level.as_deref();

        for (i, step) in steps.into_iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }

            let res = self
                .execute_single_interval_step(
                    &step,
                    &prompt,
                    &table_name,
                    &db,
                    &model_fields,
                    &target_field_names,
                    req.provider.as_deref(),
                    req.model.as_deref(),
                    req.temperature,
                    req.preamble.as_deref(),
                    enable_grounding,
                    thinking_level,
                    req.base_url.as_deref(),
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

    type PopulateTableIntervalStreamStream =
        ReceiverStream<Result<IntervalIterationResult, Status>>;

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
            let thinking_level = req.thinking_level.as_deref();

            for (i, step) in steps.into_iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }

                let res = this
                    .execute_single_interval_step(
                        &step,
                        &prompt,
                        &table_name,
                        &db,
                        &model_fields,
                        &target_field_names,
                        req.provider.as_deref(),
                        req.model.as_deref(),
                        req.temperature,
                        req.preamble.as_deref(),
                        enable_grounding,
                        thinking_level,
                        req.base_url.as_deref(),
                    )
                    .await;

                if tx.send(Ok(res)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn fact_check_entries(
        &self,
        request: Request<FactCheckEntriesRequest>,
    ) -> Result<Response<FactCheckEntriesResponse>, Status> {
        let req = request.into_inner();
        let table_name = req.table_name.trim().to_string();

        let (db, records) = self.fetch_records_for_fact_check(&req).await?;
        let total_found = records.len();
        tracing::info!(table = %table_name, count = total_found, "Starting batch fact-check");

        let update_db_status = req.update_db_status.unwrap_or(true);
        let provider = req.provider.as_deref();
        let model = req.model.as_deref();
        let temperature = req.temperature;
        let preamble = req.preamble.as_deref();
        let thinking_level = req.thinking_level.as_deref();
        let base_url = req.base_url.as_deref();

        let mut results = Vec::with_capacity(total_found);
        let mut confirmed_count = 0u64;
        let mut disputed_count = 0u64;
        let mut unverifiable_count = 0u64;
        let mut failed_count = 0u64;
        let mut checked_status_count = 0u64;
        let mut discarded_status_count = 0u64;

        for (i, record) in records.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }

            let res = self
                .execute_single_fact_check(
                    &db,
                    &table_name,
                    record,
                    update_db_status,
                    provider,
                    model,
                    temperature,
                    preamble,
                    thinking_level,
                    base_url,
                )
                .await;

            if res.success {
                match res.verdict.as_str() {
                    "confirmed" => confirmed_count += 1,
                    "disputed" => disputed_count += 1,
                    _ => unverifiable_count += 1,
                }
                match res.status.as_str() {
                    "Checked" => checked_status_count += 1,
                    _ => discarded_status_count += 1,
                }
            } else {
                failed_count += 1;
            }

            results.push(res);
        }

        let total_checked = results.len() as u64;
        let response = FactCheckEntriesResponse {
            success: failed_count == 0,
            table_name: table_name.clone(),
            total_checked,
            confirmed_count,
            disputed_count,
            unverifiable_count,
            failed_count,
            checked_status_count,
            discarded_status_count,
            results,
            message: format!(
                "Fact-checked {total_checked} records from table '{table_name}': {confirmed_count} confirmed (status: Checked), {disputed_count} disputed, {unverifiable_count} unverifiable (status: Discarded_Irrelevant), {failed_count} failed."
            ),
        };

        Ok(Response::new(response))
    }

    type FactCheckEntriesStreamStream =
        ReceiverStream<Result<FactCheckResult, Status>>;

    async fn fact_check_entries_stream(
        &self,
        request: Request<FactCheckEntriesRequest>,
    ) -> Result<Response<Self::FactCheckEntriesStreamStream>, Status> {
        let req = request.into_inner();
        let table_name = req.table_name.trim().to_string();

        let (db, records) = self.fetch_records_for_fact_check(&req).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(16);

        let this = Arc::new(TablePopulatorServiceImpl {
            config: Arc::clone(&self.config),
            db: Arc::clone(&self.db),
        });

        tokio::spawn(async move {
            let update_db_status = req.update_db_status.unwrap_or(true);
            let provider = req.provider.as_deref();
            let model = req.model.as_deref();
            let temperature = req.temperature;
            let preamble = req.preamble.as_deref();
            let thinking_level = req.thinking_level.as_deref();
            let base_url = req.base_url.as_deref();

            for (i, record) in records.iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }

                let res = this
                    .execute_single_fact_check(
                        &db,
                        &table_name,
                        record,
                        update_db_status,
                        provider,
                        model,
                        temperature,
                        preamble,
                        thinking_level,
                        base_url,
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
                if allowed_keys.contains("incident_date") && !map.contains_key("incident_date")
                    && let Some(val) = map.remove("date").or_else(|| map.remove("crime_date")) {
                        map.insert("incident_date".to_string(), val);
                    }

                // 2. Alias link / source_url -> url if allowed and missing
                if allowed_keys.contains("url") && !map.contains_key("url")
                    && let Some(val) = map.remove("link").or_else(|| map.remove("source_url")) {
                        map.insert("url".to_string(), val);
                    }

                // 3. Alias query -> discovery_query if allowed and missing
                if allowed_keys.contains("discovery_query") && !map.contains_key("discovery_query")
                    && let Some(val) = map.remove("query") {
                        map.insert("discovery_query".to_string(), val);
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

                    if let Some(title) = title_val
                        && let Some(title_str) = title.as_str() {
                            if let Some(existing_raw) = map.get_mut("raw_text") {
                                if let Some(raw_str) = existing_raw.as_str()
                                    && !raw_str.contains(title_str) {
                                        *existing_raw =
                                            serde_json::json!(format!("{}: {}", title_str, raw_str));
                                    }
                            } else {
                                map.insert(
                                    "raw_text".to_string(),
                                    serde_json::Value::String(title_str.to_string()),
                                );
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
