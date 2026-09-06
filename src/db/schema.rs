use crate::db::AppDb;
use surrealdb::{Connection, Surreal};

pub trait SchemaIntrospectable {
    fn db_info(&self) -> impl std::future::Future<Output = Result<serde_json::Value, Box<dyn std::error::Error>>> + Send;
    fn table_info(&self, table: &str) -> impl std::future::Future<Output = Result<serde_json::Value, Box<dyn std::error::Error>>> + Send;
    fn defined_tables(&self) -> impl std::future::Future<Output = Result<Vec<String>, Box<dyn std::error::Error>>> + Send;
    fn execute_query(&self, sql: &str) -> impl std::future::Future<Output = Result<serde_json::Value, Box<dyn std::error::Error>>> + Send;
}

impl<C: Connection + Send + Sync> SchemaIntrospectable for Surreal<C> {
    async fn db_info(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut response = self.query("INFO FOR DB").await?;
        let info: Option<serde_json::Value> = response.take(0)?;
        Ok(info.unwrap_or(serde_json::Value::Null))
    }

    async fn table_info(&self, table: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut response = self.query(format!("INFO FOR TABLE {table}")).await?;
        let info: Option<serde_json::Value> = response.take(0)?;
        Ok(info.unwrap_or(serde_json::Value::Null))
    }

    async fn defined_tables(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let info = self.db_info().await?;
        if let Some(tables_obj) = info.get("tables").and_then(|t| t.as_object()) {
            Ok(tables_obj.keys().cloned().collect())
        } else {
            Ok(Vec::new())
        }
    }

    async fn execute_query(&self, sql: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut response = self.query(sql).await?;
        let val: Option<serde_json::Value> = response.take(0)?;
        Ok(val.unwrap_or(serde_json::Value::Null))
    }
}

impl SchemaIntrospectable for AppDb {
    async fn db_info(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.db_info().await,
            AppDb::Remote(remote) => remote.query_raw("INFO FOR DB").await,
        }
    }

    async fn table_info(&self, table: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.table_info(table).await,
            AppDb::Remote(remote) => remote.query_raw(&format!("INFO FOR TABLE {table}")).await,
        }
    }

    async fn defined_tables(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let info = self.db_info().await?;
        if let Some(tables_obj) = info.get("tables").and_then(|t| t.as_object()) {
            Ok(tables_obj.keys().cloned().collect())
        } else {
            Ok(Vec::new())
        }
    }

    async fn execute_query(&self, sql: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.execute_query(sql).await,
            AppDb::Remote(remote) => remote.query_raw(sql).await,
        }
    }
}

/// Retrieves the database schema information using `INFO FOR DB`.
pub async fn get_db_info<T: SchemaIntrospectable>(db: &T) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    db.db_info().await
}

/// Retrieves the schema definition for a specific table using `INFO FOR TABLE <table_name>`.
pub async fn get_table_info<T: SchemaIntrospectable>(
    db: &T,
    table: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    db.table_info(table).await
}

/// Helper to get the list of defined table names from the database schema.
pub async fn get_defined_tables<T: SchemaIntrospectable>(db: &T) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    db.defined_tables().await
}

/// Executes arbitrary SurrealQL queries (including DDL statements like `DEFINE TABLE` and `DEFINE FIELD`).
pub async fn execute_surrealql<T: SchemaIntrospectable>(
    db: &T,
    sql: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    db.execute_query(sql).await
}

/// Sets or updates the COMMENT on a SurrealDB table to document the prompt used.
pub async fn set_table_comment<T: SchemaIntrospectable>(
    db: &T,
    table: &str,
    comment: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let escaped = comment.replace('\\', "\\\\").replace('\'', "\\'");
    let sql = format!("DEFINE TABLE OVERWRITE {table} COMMENT '{escaped}';");
    db.execute_query(&sql).await?;
    Ok(())
}

/// Metadata summary for a table schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TableSchemaSummary {
    pub table_name: String,
    pub comment: String,
    pub fields: Vec<String>,
    pub field_names: Vec<String>,
    pub raw_info: serde_json::Value,
}

/// Introspects table schema and extracts field names and the table comment.
pub async fn get_table_schema_summary<T: SchemaIntrospectable>(
    db: &T,
    table: &str,
) -> Result<TableSchemaSummary, Box<dyn std::error::Error>> {
    let raw_info = db.table_info(table).await?;
    let db_info = db.db_info().await?;

    let mut fields = Vec::new();
    let mut field_names = Vec::new();
    if let Some(fields_obj) = raw_info.get("fields").and_then(|f| f.as_object()) {
        for (field_name, field_val) in fields_obj {
            field_names.push(field_name.clone());
            if let Some(def_str) = field_val.as_str() {
                fields.push(format!("{field_name} ({def_str})"));
            } else {
                fields.push(field_name.clone());
            }
        }
    }

    let mut comment = String::new();
    if let Some(tables_obj) = db_info.get("tables").and_then(|t| t.as_object()) {
        if let Some(tbl_def) = tables_obj.get(table) {
            let def_str = tbl_def.as_str().unwrap_or_default();
            if let Some(c_idx) = def_str.find("COMMENT '") {
                let rest = &def_str[c_idx + 9..];
                if let Some(end_idx) = rest.rfind('\'') {
                    comment = rest[..end_idx].to_string();
                }
            }
        }
    }

    Ok(TableSchemaSummary {
        table_name: table.to_string(),
        comment,
        fields,
        field_names,
        raw_info,
    })
}
