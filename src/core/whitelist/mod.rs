pub mod in_memory;
pub mod persisted;

use std::sync::Arc;

use bittorrent_primitives::info_hash::InfoHash;
use in_memory::InMemoryWhitelist;
use persisted::DatabaseWhitelist;

use super::databases::{self, Database};

/// It handles the list of allowed torrents. Only for listed trackers.
pub struct WhiteListManager {
    /// The in-memory list of allowed torrents.
    in_memory_whitelist: InMemoryWhitelist,

    /// The persisted list of allowed torrents.
    database_whitelist: DatabaseWhitelist,
}

impl WhiteListManager {
    #[must_use]
    pub fn new(database: Arc<Box<dyn Database>>) -> Self {
        Self {
            in_memory_whitelist: InMemoryWhitelist::default(),
            database_whitelist: DatabaseWhitelist::new(database),
        }
    }

    /// It adds a torrent to the whitelist.
    /// Adding torrents is not relevant to public trackers.
    ///
    /// # Errors
    ///
    /// Will return a `database::Error` if unable to add the `info_hash` into the whitelist database.
    pub async fn add_torrent_to_whitelist(&self, info_hash: &InfoHash) -> Result<(), databases::error::Error> {
        self.database_whitelist.add(info_hash)?;
        self.in_memory_whitelist.add(info_hash).await;
        Ok(())
    }

    /// It removes a torrent from the whitelist.
    /// Removing torrents is not relevant to public trackers.
    ///
    /// # Errors
    ///
    /// Will return a `database::Error` if unable to remove the `info_hash` from the whitelist database.
    pub async fn remove_torrent_from_whitelist(&self, info_hash: &InfoHash) -> Result<(), databases::error::Error> {
        self.database_whitelist.remove(info_hash)?;
        self.in_memory_whitelist.remove(info_hash).await;
        Ok(())
    }

    /// It removes a torrent from the whitelist in the database.
    ///
    /// # Errors
    ///
    /// Will return a `database::Error` if unable to remove the `info_hash` from the whitelist database.
    pub fn remove_torrent_from_database_whitelist(&self, info_hash: &InfoHash) -> Result<(), databases::error::Error> {
        self.database_whitelist.remove(info_hash)
    }

    /// It adds a torrent from the whitelist in memory.
    pub async fn add_torrent_to_memory_whitelist(&self, info_hash: &InfoHash) -> bool {
        self.in_memory_whitelist.add(info_hash).await
    }

    /// It removes a torrent from the whitelist in memory.
    pub async fn remove_torrent_from_memory_whitelist(&self, info_hash: &InfoHash) -> bool {
        self.in_memory_whitelist.remove(info_hash).await
    }

    /// It checks if a torrent is whitelisted.
    pub async fn is_info_hash_whitelisted(&self, info_hash: &InfoHash) -> bool {
        self.in_memory_whitelist.contains(info_hash).await
    }

    /// It loads the whitelist from the database.
    ///
    /// # Errors
    ///
    /// Will return a `database::Error` if unable to load the list whitelisted `info_hash`s from the database.
    pub async fn load_whitelist_from_database(&self) -> Result<(), databases::error::Error> {
        let whitelisted_torrents_from_database = self.database_whitelist.load_from_database()?;

        self.in_memory_whitelist.clear().await;

        for info_hash in whitelisted_torrents_from_database {
            let _: bool = self.in_memory_whitelist.add(&info_hash).await;
        }

        Ok(())
    }
}
