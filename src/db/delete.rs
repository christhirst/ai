use crate::db::{TABLE_GDP, TABLE_HOMECIDES};
use crate::prompt_typed::{GdpRecord, Homecides};
use surrealdb::{Connection, Result, Surreal};

/// Helper to treat 'table does not exist' as an empty result set.
fn handle_not_found(err: surrealdb::Error) -> Result<Vec<GdpRecord>> {
    if err.to_string().contains("does not exist") {
        Ok(Vec::new())
    } else {
        Err(err)
    }
}

fn handle_not_found_homecides(err: surrealdb::Error) -> Result<Vec<Homecides>> {
    if err.to_string().contains("does not exist") {
        Ok(Vec::new())
    } else {
        Err(err)
    }
}

/// Deletes all `GdpRecord` entries from the database.
pub async fn delete_all_gdp_records<C: Connection>(db: &Surreal<C>) -> Result<Vec<GdpRecord>> {
    match db.delete(TABLE_GDP).await {
        Ok(records) => Ok(records),
        Err(err) => handle_not_found(err),
    }
}

/// Deletes `GdpRecord` entries matching a specific calendar year, returning the deleted records.
pub async fn delete_gdp_records_by_year<C: Connection>(
    db: &Surreal<C>,
    year: &str,
) -> Result<Vec<GdpRecord>> {
    let mut response = match db
        .query("DELETE FROM gdp_record WHERE year = $year RETURN BEFORE")
        .bind(("year", year.to_string()))
        .await
    {
        Ok(res) => res,
        Err(err) => return handle_not_found(err),
    };
    match response.take(0) {
        Ok(records) => Ok(records),
        Err(err) => handle_not_found(err),
    }
}

/// Deletes all `Homecides` entries from the database.
pub async fn delete_all_homecides<C: Connection>(db: &Surreal<C>) -> Result<Vec<Homecides>> {
    match db.delete(TABLE_HOMECIDES).await {
        Ok(records) => Ok(records),
        Err(err) => handle_not_found_homecides(err),
    }
}

/// Deletes `Homecides` entries matching a specific source, returning the deleted records.
pub async fn delete_homecides_by_source<C: Connection>(
    db: &Surreal<C>,
    source: &str,
) -> Result<Vec<Homecides>> {
    let mut response = match db
        .query("DELETE FROM homecides WHERE source = $source RETURN BEFORE")
        .bind(("source", source.to_string()))
        .await
    {
        Ok(res) => res,
        Err(err) => return handle_not_found_homecides(err),
    };
    match response.take(0) {
        Ok(records) => Ok(records),
        Err(err) => handle_not_found_homecides(err),
    }
}
