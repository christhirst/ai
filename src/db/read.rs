use crate::db::{AppDb, TABLE_GDP, TABLE_HOMECIDES};
use crate::prompt_typed::{GdpRecord, Homecides};
use surrealdb::{Connection, Surreal};

pub trait DbReadable {
    fn all_gdp_records(&self) -> impl std::future::Future<Output = Result<Vec<GdpRecord>, Box<dyn std::error::Error>>> + Send;
    fn gdp_records_by_year(&self, year: &str) -> impl std::future::Future<Output = Result<Vec<GdpRecord>, Box<dyn std::error::Error>>> + Send;
    fn all_homecides(&self) -> impl std::future::Future<Output = Result<Vec<Homecides>, Box<dyn std::error::Error>>> + Send;
    fn homecides_by_city(&self, city: &str) -> impl std::future::Future<Output = Result<Vec<Homecides>, Box<dyn std::error::Error>>> + Send;
    fn homecides_by_citizenship(&self, citizenship: &str) -> impl std::future::Future<Output = Result<Vec<Homecides>, Box<dyn std::error::Error>>> + Send;
    fn homecides_by_weapon(&self, weapon: &str) -> impl std::future::Future<Output = Result<Vec<Homecides>, Box<dyn std::error::Error>>> + Send;
}

impl<C: Connection + Send + Sync> DbReadable for Surreal<C> {
    async fn all_gdp_records(&self) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        match self.select(TABLE_GDP).await {
            Ok(records) => Ok(records),
            Err(e) if e.to_string().contains("does not exist") => Ok(Vec::new()),
            Err(e) => Err(Box::new(e)),
        }
    }

    async fn gdp_records_by_year(&self, year: &str) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        let mut response = self
            .query("SELECT * FROM gdp_record WHERE year = $year")
            .bind(("year", year.to_string()))
            .await?;
        let records: Vec<GdpRecord> = response.take(0).unwrap_or_default();
        Ok(records)
    }

    async fn all_homecides(&self) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self.select(TABLE_HOMECIDES).await {
            Ok(records) => Ok(records),
            Err(e) if e.to_string().contains("does not exist") => Ok(Vec::new()),
            Err(e) => Err(Box::new(e)),
        }
    }

    async fn homecides_by_city(&self, city: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        let mut response = self
            .query("SELECT * FROM homecides WHERE citiy = $city")
            .bind(("city", city.to_string()))
            .await?;
        let records: Vec<Homecides> = response.take(0).unwrap_or_default();
        Ok(records)
    }

    async fn homecides_by_citizenship(&self, citizenship: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        let mut response = self
            .query("SELECT * FROM homecides WHERE Citizenship = $citizenship")
            .bind(("citizenship", citizenship.to_string()))
            .await?;
        let records: Vec<Homecides> = response.take(0).unwrap_or_default();
        Ok(records)
    }

    async fn homecides_by_weapon(&self, weapon: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        let mut response = self
            .query("SELECT * FROM homecides WHERE weapon = $weapon")
            .bind(("weapon", weapon.to_string()))
            .await?;
        let records: Vec<Homecides> = response.take(0).unwrap_or_default();
        Ok(records)
    }
}

impl DbReadable for AppDb {
    async fn all_gdp_records(&self) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.all_gdp_records().await,
            AppDb::Remote(remote) => {
                let res = remote.query_raw("SELECT * FROM gdp_record").await?;
                let records: Vec<GdpRecord> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn gdp_records_by_year(&self, year: &str) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.gdp_records_by_year(year).await,
            AppDb::Remote(remote) => {
                let sql = format!("SELECT * FROM gdp_record WHERE year = '{year}'");
                let res = remote.query_raw(&sql).await?;
                let records: Vec<GdpRecord> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn all_homecides(&self) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.all_homecides().await,
            AppDb::Remote(remote) => {
                let res = remote.query_raw("SELECT * FROM homecides").await?;
                let records: Vec<Homecides> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn homecides_by_city(&self, city: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.homecides_by_city(city).await,
            AppDb::Remote(remote) => {
                let escaped = city.replace('\'', "\\'");
                let sql = format!("SELECT * FROM homecides WHERE citiy = '{escaped}'");
                let res = remote.query_raw(&sql).await?;
                let records: Vec<Homecides> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn homecides_by_citizenship(&self, citizenship: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.homecides_by_citizenship(citizenship).await,
            AppDb::Remote(remote) => {
                let escaped = citizenship.replace('\'', "\\'");
                let sql = format!("SELECT * FROM homecides WHERE Citizenship = '{escaped}'");
                let res = remote.query_raw(&sql).await?;
                let records: Vec<Homecides> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }

    async fn homecides_by_weapon(&self, weapon: &str) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => local.homecides_by_weapon(weapon).await,
            AppDb::Remote(remote) => {
                let escaped = weapon.replace('\'', "\\'");
                let sql = format!("SELECT * FROM homecides WHERE weapon = '{escaped}'");
                let res = remote.query_raw(&sql).await?;
                let records: Vec<Homecides> = serde_json::from_value(res).unwrap_or_default();
                Ok(records)
            }
        }
    }
}

/// Retrieves all `GdpRecord` entries from the database.
pub async fn get_all_gdp_records<T: DbReadable>(db: &T) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
    db.all_gdp_records().await
}

/// Retrieves all `GdpRecord` entries matching a specific calendar year.
pub async fn get_gdp_records_by_year<T: DbReadable>(
    db: &T,
    year: &str,
) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
    db.gdp_records_by_year(year).await
}

/// Retrieves all `Homecides` entries from the database.
pub async fn get_all_homecides<T: DbReadable>(db: &T) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    db.all_homecides().await
}

/// Retrieves all `Homecides` entries matching a specific city.
pub async fn get_homecides_by_city<T: DbReadable>(
    db: &T,
    city: &str,
) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    db.homecides_by_city(city).await
}

/// Retrieves all `Homecides` entries matching a specific citizenship.
pub async fn get_homecides_by_citizenship<T: DbReadable>(
    db: &T,
    citizenship: &str,
) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    db.homecides_by_citizenship(citizenship).await
}

/// Retrieves all `Homecides` entries matching a specific weapon.
pub async fn get_homecides_by_weapon<T: DbReadable>(
    db: &T,
    weapon: &str,
) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    db.homecides_by_weapon(weapon).await
}
