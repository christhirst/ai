use crate::db::{AppDb, TABLE_GDP, TABLE_HOMECIDES};
use crate::prompt_typed::{GdpRecord, Homecides};
use surrealdb::{Connection, Surreal};

pub trait DbDeletable {
    fn delete_all_gdps(&self) -> impl std::future::Future<Output = Result<Vec<GdpRecord>, Box<dyn std::error::Error>>> + Send;
    fn delete_gdps_by_year(&self, year: &str) -> impl std::future::Future<Output = Result<Vec<GdpRecord>, Box<dyn std::error::Error>>> + Send;
    fn delete_all_homecides(&self) -> impl std::future::Future<Output = Result<Vec<Homecides>, Box<dyn std::error::Error>>> + Send;
    fn delete_homecides_by_city(&self, city: &str) -> impl std::future::Future<Output = Result<Vec<Homecides>, Box<dyn std::error::Error>>> + Send;
    fn delete_homecides_by_citizenship(&self, citizenship: &str) -> impl std::future::Future<Output = Result<Vec<Homecides>, Box<dyn std::error::Error>>> + Send;
}

impl<C: Connection + Send + Sync> DbDeletable for Surreal<C> {
    async fn delete_all_gdps(&self) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        match self.delete(TABLE_GDP).await {
            Ok(records) => Ok(records),
            Err(e) if e.to_string().contains("does not exist") => Ok(Vec::new()),
            Err(e) => Err(Box::new(e)),
        }
    }

    async fn delete_gdps_by_year(&self, year: &str) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        let mut response = self
            .query("DELETE FROM gdp_record WHERE year = $year RETURN BEFORE")
            .bind(("year", year.to_string()))
            .await?;
        let records: Vec<GdpRecord> = response.take(0).unwrap_or_default();
        Ok(records)
    }

    async fn delete_all_homecides(&self) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self.delete(TABLE_HOMECIDES).await {
            Ok(records) => Ok(records),
            Err(e) if e.to_string().contains("does not exist") => Ok(Vec::new()),
            Err(e) => Err(Box::new(e)),
        }
    }

    async fn delete_homecides_by_city(&self, city: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        let mut response = self
            .query("DELETE FROM homecides WHERE citiy = $city RETURN BEFORE")
            .bind(("city", city.to_string()))
            .await?;
        let records: Vec<Homecides> = response.take(0).unwrap_or_default();
        Ok(records)
    }

    async fn delete_homecides_by_citizenship(&self, citizenship: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        let mut response = self
            .query("DELETE FROM homecides WHERE Citizenship = $citizenship RETURN BEFORE")
            .bind(("citizenship", citizenship.to_string()))
            .await?;
        let records: Vec<Homecides> = response.take(0).unwrap_or_default();
        Ok(records)
    }
}

impl DbDeletable for AppDb {
    async fn delete_all_gdps(&self) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.delete_all_gdps().await,
            AppDb::Remote(remote) => {
                let res = remote.query_raw("DELETE FROM gdp_record RETURN BEFORE").await?;
                let records: Vec<GdpRecord> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn delete_gdps_by_year(&self, year: &str) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.delete_gdps_by_year(year).await,
            AppDb::Remote(remote) => {
                let sql = format!("DELETE FROM gdp_record WHERE year = '{year}'");
                let res = remote.query_raw(&sql).await?;
                let records: Vec<GdpRecord> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn delete_all_homecides(&self) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.delete_all_homecides().await,
            AppDb::Remote(remote) => {
                let res = remote.query_raw("DELETE FROM homecides RETURN BEFORE").await?;
                let records: Vec<Homecides> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn delete_homecides_by_city(&self, city: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.delete_homecides_by_city(city).await,
            AppDb::Remote(remote) => {
                let escaped = city.replace('\'', "\\'");
                let sql = format!("DELETE FROM homecides WHERE citiy = '{escaped}' RETURN BEFORE");
                let res = remote.query_raw(&sql).await?;
                let records: Vec<Homecides> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn delete_homecides_by_citizenship(&self, citizenship: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.delete_homecides_by_citizenship(citizenship).await,
            AppDb::Remote(remote) => {
                let escaped = citizenship.replace('\'', "\\'");
                let sql = format!("DELETE FROM homecides WHERE Citizenship = '{escaped}' RETURN BEFORE");
                let res = remote.query_raw(&sql).await?;
                let records: Vec<Homecides> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }
}

/// Deletes all `GdpRecord` entries from the database.
pub async fn delete_all_gdp_records<T: DbDeletable>(db: &T) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
    db.delete_all_gdps().await
}

/// Deletes `GdpRecord` entries matching a specific calendar year, returning the deleted records.
pub async fn delete_gdp_records_by_year<T: DbDeletable>(
    db: &T,
    year: &str,
) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
    db.delete_gdps_by_year(year).await
}

/// Deletes all `Homecides` entries from the database.
pub async fn delete_all_homecides<T: DbDeletable>(db: &T) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    db.delete_all_homecides().await
}

/// Deletes `Homecides` entries matching a specific city, returning the deleted records.
pub async fn delete_homecides_by_city<T: DbDeletable>(
    db: &T,
    city: &str,
) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    db.delete_homecides_by_city(city).await
}

/// Deletes `Homecides` entries matching a specific citizenship, returning the deleted records.
pub async fn delete_homecides_by_citizenship<T: DbDeletable>(
    db: &T,
    citizenship: &str,
) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    db.delete_homecides_by_citizenship(citizenship).await
}
