pub mod delete;
pub mod init;
pub mod read;
pub mod write;

pub use delete::*;
pub use init::*;
pub use read::*;
pub use write::*;

pub const TABLE_GDP: &str = "gdp_record";
pub const TABLE_HOMECIDES: &str = "homecides";
