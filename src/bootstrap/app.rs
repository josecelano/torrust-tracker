//! Setup for the main tracker application.
//!
//! The [`setup`] only builds the application and its dependencies but it does not start the application.
//! In fact, there is no such thing as the main application process. When the application starts, the only thing it does is
//! starting a bunch of independent jobs. If you are looking for how things are started you should read [`app::start`](crate::app::start)
//! function documentation.
//!
//! Setup steps:
//!
//! 1. Load the global application configuration.
//! 2. Initialize static variables.
//! 3. Initialize logging.
//! 4. Initialize the domain tracker.
use std::sync::Arc;

use bittorrent_tracker_core::announce_handler::AnnounceHandler;
use bittorrent_tracker_core::authentication::handler::KeysHandler;
use bittorrent_tracker_core::authentication::key::repository::in_memory::InMemoryKeyRepository;
use bittorrent_tracker_core::authentication::key::repository::persisted::DatabaseKeyRepository;
use bittorrent_tracker_core::authentication::service;
use bittorrent_tracker_core::databases::setup::initialize_database;
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use bittorrent_tracker_core::torrent::manager::TorrentsManager;
use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;
use bittorrent_tracker_core::torrent::repository::persisted::DatabasePersistentTorrentRepository;
use bittorrent_tracker_core::whitelist::authorization::WhitelistAuthorization;
use bittorrent_tracker_core::whitelist::repository::in_memory::InMemoryWhitelist;
use bittorrent_tracker_core::whitelist::setup::initialize_whitelist_manager;
use bittorrent_udp_tracker_core::crypto::ephemeral_instance_keys;
use bittorrent_udp_tracker_core::crypto::keys::{self, Keeper as _};
use bittorrent_udp_tracker_core::services::banning::BanService;
use bittorrent_udp_tracker_core::MAX_CONNECTION_ID_ERRORS_PER_IP;
use tokio::sync::RwLock;
use torrust_tracker_clock::static_time;
use torrust_tracker_configuration::validator::Validator;
use torrust_tracker_configuration::{logging, Configuration, Core, HttpApi, UdpTracker};
use tracing::instrument;

use super::config::initialize_configuration;
use crate::container::{AppContainer, HttpApiContainer, UdpTrackerContainer};

/// It loads the configuration from the environment and builds app container.
///
/// # Panics
///
/// Setup can file if the configuration is invalid.
#[must_use]
#[instrument(skip())]
pub fn setup() -> (Configuration, AppContainer) {
    #[cfg(not(test))]
    check_seed();

    let configuration = initialize_configuration();

    if let Err(e) = configuration.validate() {
        panic!("Configuration error: {e}");
    }

    initialize_global_services(&configuration);

    tracing::info!("Configuration:\n{}", configuration.clone().mask_secrets().to_json());

    let app_container = initialize_app_container(&configuration);

    (configuration, app_container)
}

/// checks if the seed is the instance seed in production.
///
/// # Panics
///
/// It would panic if the seed is not the instance seed.
pub fn check_seed() {
    let seed = keys::Current::get_seed();
    let instance = keys::Instance::get_seed();

    assert_eq!(seed, instance, "maybe using zeroed seed in production!?");
}

/// It initializes the global services.
#[instrument(skip())]
pub fn initialize_global_services(configuration: &Configuration) {
    initialize_static();
    logging::setup(&configuration.logging);
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
    let authentication_service = Arc::new(service::AuthenticationService::new(
        &configuration.core,
        &in_memory_key_repository,
    ));
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

/// It initializes the application static values.
///
/// These values are accessible throughout the entire application:
///
/// - The time when the application started.
/// - An ephemeral instance random seed. This seed is used for encryption and it's changed when the main application process is restarted.
#[instrument(skip())]
pub fn initialize_static() {
    // Set the time of Torrust app starting
    lazy_static::initialize(&static_time::TIME_AT_APP_START);

    // Initialize the Ephemeral Instance Random Seed
    lazy_static::initialize(&ephemeral_instance_keys::RANDOM_SEED);

    // Initialize the Ephemeral Instance Random Cipher
    lazy_static::initialize(&ephemeral_instance_keys::RANDOM_CIPHER_BLOWFISH);

    // Initialize the Zeroed Cipher
    lazy_static::initialize(&ephemeral_instance_keys::ZEROED_TEST_CIPHER_BLOWFISH);
}
