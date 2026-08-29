use crate::db::AppDb;
use surrealdb::{Connection, Surreal};

pub trait SchemaIntrospectable {
    fn db_info(&self) -> impl std::future::Future<Output = Result<serde_json::Value, Box<dyn std::error::Error>>> + Send;
    fn table_info(&self, table: &str) -> impl std::future::Future<Output = Result<serde_json::Value, Box<dyn std::error::Error>>> + Send;
    fn defined_tables(&self) -> impl std::future::Future<Output = Result<Vec<String>, Box<dyn std::error::Error>>> + Send;
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
