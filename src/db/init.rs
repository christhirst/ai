use crate::config::DatabaseConfig;
use surrealdb::engine::any::Any;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::auth::Root;
use surrealdb::{Result, Surreal};

/// Type alias for local SurrealDB instance.
pub type LocalDb = Surreal<Db>;

/// Type alias for dynamic SurrealDB instance (supports local and remote engines).
pub type AnyDb = Surreal<Any>;

/// Initializes an in-memory SurrealDB database instance with a given namespace and database.
pub async fn init_memory_db(namespace: &str, database: &str) -> Result<LocalDb> {
    let db = Surreal::new::<Mem>(()).await?;
    db.use_ns(namespace).use_db(database).await?;
    Ok(db)
}

/// Initializes a SurrealDB database connection based on the given DatabaseConfig.
/// Dynamically connects to the configured endpoint (e.g. "mem://", "ws://localhost:8000", "http://localhost:8000").
pub async fn init_db_from_config(config: &DatabaseConfig) -> Result<AnyDb> {
    let db = surrealdb::engine::any::connect(&config.endpoint).await?;
    db.use_ns(&config.namespace)
        .use_db(&config.database)
        .await?;
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        db.signin(Root {
            username: username.clone(),
            password: password.clone(),
        })
        .await?;
    }
    Ok(db)
}
