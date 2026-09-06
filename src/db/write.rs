use crate::db::{AppDb, TABLE_GDP, TABLE_HOMECIDES};
use crate::prompt_typed::{GdpRecord, Homecides};
use surrealdb::{Connection, Surreal};

pub trait DbWritable {
    fn create_homecide(&self, record: &Homecides) -> impl std::future::Future<Output = Result<Option<Homecides>, Box<dyn std::error::Error>>> + Send;
    fn create_homecides(&self, records: &[Homecides]) -> impl std::future::Future<Output = Result<Vec<Homecides>, Box<dyn std::error::Error>>> + Send;
    fn create_gdp(&self, record: &GdpRecord) -> impl std::future::Future<Output = Result<Option<GdpRecord>, Box<dyn std::error::Error>>> + Send;
    fn create_gdps(&self, records: &[GdpRecord]) -> impl std::future::Future<Output = Result<Vec<GdpRecord>, Box<dyn std::error::Error>>> + Send;
    fn create_dynamic_records(&self, table: &str, records: &[serde_json::Value]) -> impl std::future::Future<Output = Result<Vec<serde_json::Value>, Box<dyn std::error::Error>>> + Send;
}

impl<C: Connection + Send + Sync> DbWritable for Surreal<C> {
    async fn create_homecide(&self, record: &Homecides) -> Result<Option<Homecides>, Box<dyn std::error::Error>> {
        let res = self.create(TABLE_HOMECIDES).content(record.clone()).await?;
        Ok(res)
    }

    async fn create_homecides(&self, records: &[Homecides]) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        let res = self.insert(TABLE_HOMECIDES).content(records.to_vec()).await?;
        Ok(res)
    }

    async fn create_gdp(&self, record: &GdpRecord) -> Result<Option<GdpRecord>, Box<dyn std::error::Error>> {
        let res = self.create(TABLE_GDP).content(record.clone()).await?;
        Ok(res)
    }

    async fn create_gdps(&self, records: &[GdpRecord]) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        let res = self.insert(TABLE_GDP).content(records.to_vec()).await?;
        Ok(res)
    }

    async fn create_dynamic_records(&self, table: &str, records: &[serde_json::Value]) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let cleaned: Vec<serde_json::Value> = records.iter().map(strip_null_fields).collect();
        let res: Vec<serde_json::Value> = self.insert(table).content(cleaned).await?;
        Ok(res)
    }
}

impl DbWritable for AppDb {
    async fn create_homecide(&self, record: &Homecides) -> Result<Option<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.create_homecide(record).await,
            AppDb::Remote(remote) => {
                let date_str = record.Date.to_string();
                let prison_str = record.Prison_time.to_string();
                let citiy_escaped = record.citiy.replace('\'', "\\'");
                let citizenship_escaped = record.Citizenship.replace('\'', "\\'");
                let weapon_escaped = record.weapon.replace('\'', "\\'");
                let other_escaped = record.Other_sentence.replace('\'', "\\'");

                let sql = format!(
                    "INSERT INTO homecides {{ citiy: '{citiy_escaped}', Citizenship: '{citizenship_escaped}', Date: <datetime>'{date_str}', weapon: '{weapon_escaped}', Prison_time: <duration>'{prison_str}', Other_sentence: '{other_escaped}' }};"
                );
                let res = remote.query_raw(&sql).await?;
                let items: Vec<Homecides> = serde_json::from_value(res)?;
                Ok(items.into_iter().next())
            }
        }
    }

    async fn create_homecides(&self, records: &[Homecides]) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.create_homecides(records).await,
            AppDb::Remote(remote) => {
                if records.is_empty() {
                    return Ok(Vec::new());
                }
                let mut values_ql = Vec::new();
                for r in records {
                    let date_str = r.Date.to_string();
                    let prison_str = r.Prison_time.to_string();
                    let citiy_escaped = r.citiy.replace('\'', "\\'");
                    let citizenship_escaped = r.Citizenship.replace('\'', "\\'");
                    let weapon_escaped = r.weapon.replace('\'', "\\'");
                    let other_escaped = r.Other_sentence.replace('\'', "\\'");
                    values_ql.push(format!(
                        "{{ citiy: '{citiy_escaped}', Citizenship: '{citizenship_escaped}', Date: <datetime>'{date_str}', weapon: '{weapon_escaped}', Prison_time: <duration>'{prison_str}', Other_sentence: '{other_escaped}' }}"
                    ));
                }
                let sql = format!("INSERT INTO homecides [{}];", values_ql.join(", "));
                let res = remote.query_raw(&sql).await?;
                let items: Vec<Homecides> = serde_json::from_value(res)?;
                Ok(items)
            }
        }
    }

    async fn create_gdp(&self, record: &GdpRecord) -> Result<Option<GdpRecord>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.create_gdp(record).await,
            AppDb::Remote(remote) => {
                let sql = format!(
                    "INSERT INTO gdp_record {{ year: '{}', gdp: {} }};",
                    record.year, record.gdp
                );
                let res = remote.query_raw(&sql).await?;
                let items: Vec<GdpRecord> = serde_json::from_value(res)?;
                Ok(items.into_iter().next())
            }
        }
    }

    async fn create_gdps(&self, records: &[GdpRecord]) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.create_gdps(records).await,
            AppDb::Remote(remote) => {
                let mut values_ql = Vec::new();
                for r in records {
                    values_ql.push(format!("{{ year: '{}', gdp: {} }}", r.year, r.gdp));
                }
                let sql = format!("INSERT INTO gdp_record [{}];", values_ql.join(", "));
                let res = remote.query_raw(&sql).await?;
                let items: Vec<GdpRecord> = serde_json::from_value(res)?;
                Ok(items)
            }
        }
    }

    async fn create_dynamic_records(&self, table: &str, records: &[serde_json::Value]) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.create_dynamic_records(table, records).await,
            AppDb::Remote(remote) => {
                if records.is_empty() {
                    return Ok(Vec::new());
                }
                let cleaned: Vec<serde_json::Value> = records.iter().map(strip_null_fields).collect();
                let data_json = serde_json::to_string(&cleaned)?;
                let coerced_json = coerce_surrealql_literals(&data_json);
                let sql = format!("INSERT INTO {table} {coerced_json};");
                let res = remote.query_raw(&sql).await?;
                let items: Vec<serde_json::Value> = serde_json::from_value(res).unwrap_or_default();
                Ok(items)
            }
        }
    }
}

/// Coerces ISO-8601 datetime strings into SurrealQL `<datetime>'...'` literals
/// so SCHEMAFULL `TYPE datetime` fields are properly coerced instead of rejected as string.
pub fn coerce_surrealql_literals(json_str: &str) -> String {
    let re = regex::Regex::new(r#""(\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?)?)""#)
        .expect("Valid datetime regex");
    re.replace_all(json_str, "<datetime>'$1'").to_string()
}

/// Recursively removes null values from JSON objects so SurrealDB treats them as NONE (absent)
/// instead of literal NULL, preventing coercion errors on `none | record<...>` schema fields.
pub fn strip_null_fields(val: &serde_json::Value) -> serde_json::Value {
    match val {
        serde_json::Value::Object(map) => {
            let filtered: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), strip_null_fields(v)))
                .collect();
            serde_json::Value::Object(filtered)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(strip_null_fields).collect())
        }
        other => other.clone(),
    }
}

/// Creates a single `GdpRecord` in the database.
pub async fn create_gdp_record<T: DbWritable>(
    db: &T,
    record: &GdpRecord,
) -> Result<Option<GdpRecord>, Box<dyn std::error::Error>> {
    db.create_gdp(record).await
}

/// Creates multiple `GdpRecord` entries in bulk in the database.
pub async fn create_gdp_records<T: DbWritable>(
    db: &T,
    records: &[GdpRecord],
) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
    db.create_gdps(records).await
}

/// Creates a single `Homecides` record in the database.
pub async fn create_homecide_record<T: DbWritable>(
    db: &T,
    record: &Homecides,
) -> Result<Option<Homecides>, Box<dyn std::error::Error>> {
    db.create_homecide(record).await
}

/// Creates multiple `Homecides` entries in bulk in the database.
pub async fn create_homecide_records<T: DbWritable>(
    db: &T,
    records: &[Homecides],
) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    db.create_homecides(records).await
}

/// Inserts dynamic/generic JSON records in bulk into the specified database table.
pub async fn insert_dynamic_records<T: DbWritable>(
    db: &T,
    table: &str,
    records: &[serde_json::Value],
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    db.create_dynamic_records(table, records).await
}
