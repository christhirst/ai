use crate::db::{TABLE_GDP, TABLE_HOMECIDES};
use crate::prompt_typed::{GdpRecord, Homecides};
use surrealdb::{Connection, Result, Surreal};

/// Creates a single `GdpRecord` in the database.
pub async fn create_gdp_record<C: Connection>(
    db: &Surreal<C>,
    record: &GdpRecord,
) -> Result<Option<GdpRecord>> {
    db.create(TABLE_GDP).content(record.clone()).await
}

/// Creates multiple `GdpRecord` entries in bulk in the database.
pub async fn create_gdp_records<C: Connection>(
    db: &Surreal<C>,
    records: &[GdpRecord],
) -> Result<Vec<GdpRecord>> {
    db.insert(TABLE_GDP).content(records.to_vec()).await
}

/// Creates a single `Homecides` record in the database.
pub async fn create_homecide_record<C: Connection>(
    db: &Surreal<C>,
    record: &Homecides,
) -> Result<Option<Homecides>> {
    db.create(TABLE_HOMECIDES).content(record.clone()).await
}

/// Creates multiple `Homecides` entries in bulk in the database.
pub async fn create_homecide_records<C: Connection>(
    db: &Surreal<C>,
    records: &[Homecides],
) -> Result<Vec<Homecides>> {
    db.insert(TABLE_HOMECIDES).content(records.to_vec()).await
}
