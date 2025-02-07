//! Database driver factory.
//!
//! See [`databases::driver::build`](crate::core::databases::driver::build)
//! function for more information.
use mysql::Mysql;
use serde::{Deserialize, Serialize};
use sqlite::Sqlite;

use super::error::Error;
use super::Database;

/// The database management system used by the tracker.
///
/// Refer to:
///
/// - [Torrust Tracker Configuration](https://docs.rs/torrust-tracker-configuration).
/// - [Torrust Tracker](https://docs.rs/torrust-tracker).
///
/// For more information about persistence.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, derive_more::Display, Clone)]
pub enum Driver {
    /// The Sqlite3 database driver.
    Sqlite3,
    /// The `MySQL` database driver.
    MySQL,
}

/// It builds a new database driver.
///
/// Example for `SQLite3`:
///
/// ```text
/// use bittorrent_tracker_core::databases;
/// use bittorrent_tracker_core::databases::driver::Driver;
///
/// let db_driver = Driver::Sqlite3;
/// let db_path = "./storage/tracker/lib/database/sqlite3.db".to_string();
/// let database = databases::driver::build(&db_driver, &db_path);
/// ```
///
/// Example for `MySQL`:
///
/// ```text
/// use bittorrent_tracker_core::databases;
/// use bittorrent_tracker_core::databases::driver::Driver;
///
/// let db_driver = Driver::MySQL;
/// let db_path = "mysql://db_user:db_user_secret_password@mysql:3306/torrust_tracker".to_string();
/// let database = databases::driver::build(&db_driver, &db_path);
/// ```
///
/// Refer to the [configuration documentation](https://docs.rs/torrust-tracker-configuration)
/// for more information about the database configuration.
///
/// > **WARNING**: The driver instantiation runs database migrations.
///
/// # Errors
///
/// This function will return an error if unable to connect to the database.
///
/// # Panics
///
/// This function will panic if unable to create database tables.
pub mod mysql;
pub mod sqlite;

/// It builds a new database driver.
///
/// # Panics
///
/// Will panic if unable to create database tables.
///
/// # Errors
///
/// Will return `Error` if unable to build the driver.
pub fn build(driver: &Driver, db_path: &str) -> Result<Box<dyn Database>, Error> {
    let database: Box<dyn Database> = match driver {
        Driver::Sqlite3 => Box::new(Sqlite::new(db_path)?),
        Driver::MySQL => Box::new(Mysql::new(db_path)?),
    };

    database.create_database_tables().expect("Could not create database tables.");

    Ok(database)
}
