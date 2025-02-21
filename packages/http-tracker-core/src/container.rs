use std::sync::Arc;

use bittorrent_tracker_core::announce_handler::AnnounceHandler;
use bittorrent_tracker_core::authentication::key::repository::in_memory::InMemoryKeyRepository;
use bittorrent_tracker_core::authentication::service::AuthenticationService;
use bittorrent_tracker_core::databases::setup::initialize_database;
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;
use bittorrent_tracker_core::torrent::repository::persisted::DatabasePersistentTorrentRepository;
use bittorrent_tracker_core::whitelist;
use bittorrent_tracker_core::whitelist::authorization::WhitelistAuthorization;
use bittorrent_tracker_core::whitelist::repository::in_memory::InMemoryWhitelist;
use torrust_tracker_configuration::{Core, HttpTracker};

use crate::statistics;

pub struct HttpTrackerContainer {
    pub core_config: Arc<Core>,
    pub http_tracker_config: Arc<HttpTracker>,
    pub announce_handler: Arc<AnnounceHandler>,
    pub scrape_handler: Arc<ScrapeHandler>,
    pub whitelist_authorization: Arc<whitelist::authorization::WhitelistAuthorization>,
    pub http_stats_event_sender: Arc<Option<Box<dyn statistics::event::sender::Sender>>>,
    pub authentication_service: Arc<AuthenticationService>,
}

impl HttpTrackerContainer {
    #[must_use]
    pub fn initialize(core_config: &Arc<Core>, http_tracker_config: &Arc<HttpTracker>) -> Arc<Self> {
        // HTTP stats
        let (http_stats_event_sender, _http_stats_repository) = statistics::setup::factory(core_config.tracker_usage_statistics);
        let http_stats_event_sender = Arc::new(http_stats_event_sender);

        let database = initialize_database(core_config);
        let in_memory_whitelist = Arc::new(InMemoryWhitelist::default());
        let whitelist_authorization = Arc::new(WhitelistAuthorization::new(core_config, &in_memory_whitelist.clone()));
        let in_memory_key_repository = Arc::new(InMemoryKeyRepository::default());
        let authentication_service = Arc::new(AuthenticationService::new(core_config, &in_memory_key_repository));

        let in_memory_torrent_repository = Arc::new(InMemoryTorrentRepository::default());
        let db_torrent_repository = Arc::new(DatabasePersistentTorrentRepository::new(&database));

        let announce_handler = Arc::new(AnnounceHandler::new(
            core_config,
            &whitelist_authorization,
            &in_memory_torrent_repository,
            &db_torrent_repository,
        ));

        let scrape_handler = Arc::new(ScrapeHandler::new(&whitelist_authorization, &in_memory_torrent_repository));

        Arc::new(Self {
            http_tracker_config: http_tracker_config.clone(),
            core_config: core_config.clone(),
            announce_handler: announce_handler.clone(),
            scrape_handler: scrape_handler.clone(),
            whitelist_authorization: whitelist_authorization.clone(),
            http_stats_event_sender: http_stats_event_sender.clone(),
            authentication_service: authentication_service.clone(),
        })
    }
}
