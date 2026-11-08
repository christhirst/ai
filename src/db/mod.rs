pub mod delete;
pub mod init;
pub mod read;
pub mod schema;
pub mod write;

pub use delete::*;
pub use init::*;
pub use read::*;
pub use schema::*;
pub use write::*;

pub const TABLE_GDP: &str = "gdp_record";
pub const TABLE_HOMECIDES: &str = "homecides";

/// Escapes backslashes and single quotes for safe interpolation into single-quoted SurrealQL literals.
pub fn escape_surreal_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}
