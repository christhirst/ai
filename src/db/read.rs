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

/// Retrieves all `GdpRecord` entries from the database.
pub async fn get_all_gdp_records<C: Connection>(db: &Surreal<C>) -> Result<Vec<GdpRecord>> {
    match db.select(TABLE_GDP).await {
        Ok(records) => Ok(records),
        Err(err) => handle_not_found(err),
    }
}

/// Retrieves all `GdpRecord` entries matching a specific calendar year.
pub async fn get_gdp_records_by_year<C: Connection>(
    db: &Surreal<C>,
    year: &str,
) -> Result<Vec<GdpRecord>> {
    let mut response = match db
        .query("SELECT * FROM gdp_record WHERE year = $year")
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

/// Retrieves all `Homecides` entries from the database.
pub async fn get_all_homecides<C: Connection>(db: &Surreal<C>) -> Result<Vec<Homecides>> {
    match db.select(TABLE_HOMECIDES).await {
        Ok(records) => Ok(records),
        Err(err) => handle_not_found_homecides(err),
    }
}

/// Retrieves all `Homecides` entries matching a specific source.
pub async fn get_homecides_by_source<C: Connection>(
    db: &Surreal<C>,
    source: &str,
) -> Result<Vec<Homecides>> {
    let mut response = match db
        .query("SELECT * FROM homecides WHERE source = $source")
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

/// Retrieves all `Homecides` entries with amount greater than or equal to `min_amount`.
pub async fn get_homecides_by_min_amount<C: Connection>(
    db: &Surreal<C>,
    min_amount: i32,
) -> Result<Vec<Homecides>> {
    let mut response = match db
        .query("SELECT * FROM homecides WHERE amount >= $min_amount")
        .bind(("min_amount", min_amount))
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
