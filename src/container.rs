use std::sync::Arc;

use bittorrent_tracker_core::announce_handler::AnnounceHandler;
use bittorrent_tracker_core::authentication::handler::KeysHandler;
use bittorrent_tracker_core::authentication::key::repository::in_memory::InMemoryKeyRepository;
use bittorrent_tracker_core::authentication::key::repository::persisted::DatabaseKeyRepository;
use bittorrent_tracker_core::authentication::service::AuthenticationService;
use bittorrent_tracker_core::databases::setup::initialize_database;
use bittorrent_tracker_core::databases::Database;
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use bittorrent_tracker_core::torrent::manager::TorrentsManager;
use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;
use bittorrent_tracker_core::torrent::repository::persisted::DatabasePersistentTorrentRepository;
use bittorrent_tracker_core::whitelist;
use bittorrent_tracker_core::whitelist::authorization::WhitelistAuthorization;
use bittorrent_tracker_core::whitelist::manager::WhitelistManager;
use bittorrent_tracker_core::whitelist::repository::in_memory::InMemoryWhitelist;
use bittorrent_tracker_core::whitelist::setup::initialize_whitelist_manager;
use bittorrent_udp_tracker_core::services::banning::BanService;
use bittorrent_udp_tracker_core::{self, MAX_CONNECTION_ID_ERRORS_PER_IP};
use tokio::sync::RwLock;
use torrust_axum_http_tracker_server::container::HttpTrackerContainer;
use torrust_tracker_configuration::{Configuration, Core, HttpApi, HttpTracker, UdpTracker};
use tracing::instrument;

pub struct AppContainer {
    pub core_config: Arc<Core>,
    pub database: Arc<Box<dyn Database>>,
    pub announce_handler: Arc<AnnounceHandler>,
    pub scrape_handler: Arc<ScrapeHandler>,
    pub keys_handler: Arc<KeysHandler>,
    pub authentication_service: Arc<AuthenticationService>,
    pub in_memory_whitelist: Arc<InMemoryWhitelist>,
    pub whitelist_authorization: Arc<whitelist::authorization::WhitelistAuthorization>,
    pub ban_service: Arc<RwLock<BanService>>,
    pub http_stats_event_sender: Arc<Option<Box<dyn bittorrent_http_tracker_core::statistics::event::sender::Sender>>>,
    pub udp_stats_event_sender: Arc<Option<Box<dyn bittorrent_udp_tracker_core::statistics::event::sender::Sender>>>,
    pub http_stats_repository: Arc<bittorrent_http_tracker_core::statistics::repository::Repository>,
    pub udp_stats_repository: Arc<bittorrent_udp_tracker_core::statistics::repository::Repository>,
    pub whitelist_manager: Arc<WhitelistManager>,
    pub in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
    pub db_torrent_repository: Arc<DatabasePersistentTorrentRepository>,
    pub torrents_manager: Arc<TorrentsManager>,
}

impl AppContainer {
    #[must_use]
    pub fn http_tracker_container(&self, http_tracker_config: &Arc<HttpTracker>) -> HttpTrackerContainer {
        HttpTrackerContainer {
            http_tracker_config: http_tracker_config.clone(),
            core_config: self.core_config.clone(),
            announce_handler: self.announce_handler.clone(),
            scrape_handler: self.scrape_handler.clone(),
            whitelist_authorization: self.whitelist_authorization.clone(),
            http_stats_event_sender: self.http_stats_event_sender.clone(),
            authentication_service: self.authentication_service.clone(),
        }
    }

    #[must_use]
    pub fn udp_tracker_container(&self, udp_tracker_config: &Arc<UdpTracker>) -> UdpTrackerContainer {
        UdpTrackerContainer {
            udp_tracker_config: udp_tracker_config.clone(),
            core_config: self.core_config.clone(),
            announce_handler: self.announce_handler.clone(),
            scrape_handler: self.scrape_handler.clone(),
            whitelist_authorization: self.whitelist_authorization.clone(),
            udp_stats_event_sender: self.udp_stats_event_sender.clone(),
            ban_service: self.ban_service.clone(),
        }
    }

    #[must_use]
    pub fn http_api_container(&self, http_api_config: &Arc<HttpApi>) -> HttpApiContainer {
        HttpApiContainer {
            http_api_config: http_api_config.clone(),
            core_config: self.core_config.clone(),
            in_memory_torrent_repository: self.in_memory_torrent_repository.clone(),
            keys_handler: self.keys_handler.clone(),
            whitelist_manager: self.whitelist_manager.clone(),
            ban_service: self.ban_service.clone(),
            http_stats_repository: self.http_stats_repository.clone(),
            udp_stats_repository: self.udp_stats_repository.clone(),
        }
    }
}

pub struct UdpTrackerContainer {
    pub core_config: Arc<Core>,
    pub udp_tracker_config: Arc<UdpTracker>,
    pub announce_handler: Arc<AnnounceHandler>,
    pub scrape_handler: Arc<ScrapeHandler>,
    pub whitelist_authorization: Arc<whitelist::authorization::WhitelistAuthorization>,
    pub udp_stats_event_sender: Arc<Option<Box<dyn bittorrent_udp_tracker_core::statistics::event::sender::Sender>>>,
    pub ban_service: Arc<RwLock<BanService>>,
}

pub struct HttpApiContainer {
    pub core_config: Arc<Core>,
    pub http_api_config: Arc<HttpApi>,
    pub in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
    pub keys_handler: Arc<KeysHandler>,
    pub whitelist_manager: Arc<WhitelistManager>,
    pub ban_service: Arc<RwLock<BanService>>,
    pub http_stats_repository: Arc<bittorrent_http_tracker_core::statistics::repository::Repository>,
    pub udp_stats_repository: Arc<bittorrent_udp_tracker_core::statistics::repository::Repository>,
}

/// It initializes the IoC Container.
#[instrument(skip())]
pub fn initialize_app_container(configuration: &Configuration) -> AppContainer {
    let core_config = Arc::new(configuration.core.clone());

    // HTTP stats
    let (http_stats_event_sender, http_stats_repository) =
        bittorrent_http_tracker_core::statistics::setup::factory(configuration.core.tracker_usage_statistics);
    let http_stats_event_sender = Arc::new(http_stats_event_sender);
    let http_stats_repository = Arc::new(http_stats_repository);

    // UDP stats
    let (udp_stats_event_sender, udp_stats_repository) =
        bittorrent_udp_tracker_core::statistics::setup::factory(configuration.core.tracker_usage_statistics);
    let udp_stats_event_sender = Arc::new(udp_stats_event_sender);
    let udp_stats_repository = Arc::new(udp_stats_repository);

    let ban_service = Arc::new(RwLock::new(BanService::new(MAX_CONNECTION_ID_ERRORS_PER_IP)));
    let database = initialize_database(&configuration.core);
    let in_memory_whitelist = Arc::new(InMemoryWhitelist::default());
    let whitelist_authorization = Arc::new(WhitelistAuthorization::new(&configuration.core, &in_memory_whitelist.clone()));
    let whitelist_manager = initialize_whitelist_manager(database.clone(), in_memory_whitelist.clone());
    let db_key_repository = Arc::new(DatabaseKeyRepository::new(&database));
    let in_memory_key_repository = Arc::new(InMemoryKeyRepository::default());
    let authentication_service = Arc::new(AuthenticationService::new(&configuration.core, &in_memory_key_repository));
    let keys_handler = Arc::new(KeysHandler::new(
        &db_key_repository.clone(),
        &in_memory_key_repository.clone(),
    ));
    let in_memory_torrent_repository = Arc::new(InMemoryTorrentRepository::default());
    let db_torrent_repository = Arc::new(DatabasePersistentTorrentRepository::new(&database));

    let torrents_manager = Arc::new(TorrentsManager::new(
        &configuration.core,
        &in_memory_torrent_repository,
        &db_torrent_repository,
    ));

    let announce_handler = Arc::new(AnnounceHandler::new(
        &configuration.core,
        &whitelist_authorization,
        &in_memory_torrent_repository,
        &db_torrent_repository,
    ));

    let scrape_handler = Arc::new(ScrapeHandler::new(&whitelist_authorization, &in_memory_torrent_repository));

    AppContainer {
        core_config,
        database,
        announce_handler,
        scrape_handler,
        keys_handler,
        authentication_service,
        in_memory_whitelist,
        whitelist_authorization,
        ban_service,
        http_stats_event_sender,
        udp_stats_event_sender,
        http_stats_repository,
        udp_stats_repository,
        whitelist_manager,
        in_memory_torrent_repository,
        db_torrent_repository,
        torrents_manager,
    }
}

#[must_use]
pub fn initialize_http_api_container(core_config: &Arc<Core>, http_api_config: &Arc<HttpApi>) -> Arc<HttpApiContainer> {
    // HTTP stats
    let (_http_stats_event_sender, http_stats_repository) =
        bittorrent_http_tracker_core::statistics::setup::factory(core_config.tracker_usage_statistics);
    let http_stats_repository = Arc::new(http_stats_repository);

    // UDP stats
    let (_udp_stats_event_sender, udp_stats_repository) =
        bittorrent_udp_tracker_core::statistics::setup::factory(core_config.tracker_usage_statistics);
    let udp_stats_repository = Arc::new(udp_stats_repository);

    let ban_service = Arc::new(RwLock::new(BanService::new(MAX_CONNECTION_ID_ERRORS_PER_IP)));
    let database = initialize_database(core_config);
    let in_memory_whitelist = Arc::new(InMemoryWhitelist::default());
    let whitelist_manager = initialize_whitelist_manager(database.clone(), in_memory_whitelist.clone());
    let db_key_repository = Arc::new(DatabaseKeyRepository::new(&database));
    let in_memory_key_repository = Arc::new(InMemoryKeyRepository::default());
    let keys_handler = Arc::new(KeysHandler::new(
        &db_key_repository.clone(),
        &in_memory_key_repository.clone(),
    ));
    let in_memory_torrent_repository = Arc::new(InMemoryTorrentRepository::default());

    Arc::new(HttpApiContainer {
        http_api_config: http_api_config.clone(),
        core_config: core_config.clone(),
        in_memory_torrent_repository: in_memory_torrent_repository.clone(),
        keys_handler: keys_handler.clone(),
        whitelist_manager: whitelist_manager.clone(),
        ban_service: ban_service.clone(),
        http_stats_repository: http_stats_repository.clone(),
        udp_stats_repository: udp_stats_repository.clone(),
    })
}

#[must_use]
pub fn initialize_udt_tracker_container(
    core_config: &Arc<Core>,
    udp_tracker_config: &Arc<UdpTracker>,
) -> Arc<UdpTrackerContainer> {
    // UDP stats
    let (udp_stats_event_sender, _udp_stats_repository) =
        bittorrent_udp_tracker_core::statistics::setup::factory(core_config.tracker_usage_statistics);
    let udp_stats_event_sender = Arc::new(udp_stats_event_sender);

    let ban_service = Arc::new(RwLock::new(BanService::new(MAX_CONNECTION_ID_ERRORS_PER_IP)));
    let database = initialize_database(core_config);
    let in_memory_whitelist = Arc::new(InMemoryWhitelist::default());
    let whitelist_authorization = Arc::new(WhitelistAuthorization::new(core_config, &in_memory_whitelist.clone()));
    let in_memory_torrent_repository = Arc::new(InMemoryTorrentRepository::default());
    let db_torrent_repository = Arc::new(DatabasePersistentTorrentRepository::new(&database));

    let announce_handler = Arc::new(AnnounceHandler::new(
        core_config,
        &whitelist_authorization,
        &in_memory_torrent_repository,
        &db_torrent_repository,
    ));

    let scrape_handler = Arc::new(ScrapeHandler::new(&whitelist_authorization, &in_memory_torrent_repository));

    Arc::new(UdpTrackerContainer {
        udp_tracker_config: udp_tracker_config.clone(),
        core_config: core_config.clone(),
        announce_handler: announce_handler.clone(),
        scrape_handler: scrape_handler.clone(),
        whitelist_authorization: whitelist_authorization.clone(),
        udp_stats_event_sender: udp_stats_event_sender.clone(),
        ban_service: ban_service.clone(),
    })
}
