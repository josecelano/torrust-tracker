use std::sync::Arc;

use bittorrent_http_tracker_core::container::HttpTrackerContainer;
use bittorrent_primitives::info_hash::InfoHash;
use bittorrent_tracker_core::announce_handler::AnnounceHandler;
use bittorrent_tracker_core::authentication::handler::KeysHandler;
use bittorrent_tracker_core::authentication::key::repository::in_memory::InMemoryKeyRepository;
use bittorrent_tracker_core::authentication::key::repository::persisted::DatabaseKeyRepository;
use bittorrent_tracker_core::authentication::service::AuthenticationService;
use bittorrent_tracker_core::databases::setup::initialize_database;
use bittorrent_tracker_core::databases::Database;
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;
use bittorrent_tracker_core::torrent::repository::persisted::DatabasePersistentTorrentRepository;
use bittorrent_tracker_core::whitelist::authorization::WhitelistAuthorization;
use bittorrent_tracker_core::whitelist::manager::WhitelistManager;
use bittorrent_tracker_core::whitelist::repository::in_memory::InMemoryWhitelist;
use bittorrent_tracker_core::whitelist::setup::initialize_whitelist_manager;
use futures::executor::block_on;
use torrust_axum_http_tracker_server::server::{HttpServer, Launcher, Running, Stopped};
use torrust_axum_server::tsl::make_rust_tls;
use torrust_server_lib::registar::Registar;
use torrust_tracker_configuration::{Configuration, Core, HttpTracker};
use torrust_tracker_lib::bootstrap::app::initialize_global_services;
use torrust_tracker_primitives::peer;

pub struct Environment<S> {
    pub http_tracker_container: Arc<HttpTrackerContainer>,

    pub database: Arc<Box<dyn Database>>,
    pub in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
    pub keys_handler: Arc<KeysHandler>,
    pub http_stats_repository: Arc<bittorrent_http_tracker_core::statistics::repository::Repository>,
    pub whitelist_manager: Arc<WhitelistManager>,

    pub registar: Registar,
    pub server: HttpServer<S>,
}

impl<S> Environment<S> {
    /// Add a torrent to the tracker
    pub fn add_torrent_peer(&self, info_hash: &InfoHash, peer: &peer::Peer) {
        let () = self.in_memory_torrent_repository.upsert_peer(info_hash, peer);
    }
}

impl Environment<Stopped> {
    #[allow(dead_code)]
    pub fn new(configuration: &Arc<Configuration>) -> Self {
        initialize_global_services(configuration);

        let env_container = EnvContainer::initialize(configuration);

        let bind_to = env_container.http_tracker_config.bind_address;

        let tls =
            block_on(make_rust_tls(&env_container.http_tracker_config.tsl_config)).map(|tls| tls.expect("tls config failed"));

        let server = HttpServer::new(Launcher::new(bind_to, tls));

        let http_tracker_container = Arc::new(HttpTrackerContainer {
            core_config: env_container.core_config.clone(),
            http_tracker_config: env_container.http_tracker_config.clone(),
            announce_handler: env_container.http_tracker_container.announce_handler.clone(),
            scrape_handler: env_container.http_tracker_container.scrape_handler.clone(),
            whitelist_authorization: env_container.http_tracker_container.whitelist_authorization.clone(),
            http_stats_event_sender: env_container.http_tracker_container.http_stats_event_sender.clone(),
            authentication_service: env_container.http_tracker_container.authentication_service.clone(),
        });

        Self {
            http_tracker_container,

            database: env_container.database.clone(),
            in_memory_torrent_repository: env_container.in_memory_torrent_repository.clone(),
            keys_handler: env_container.keys_handler.clone(),
            http_stats_repository: env_container.http_stats_repository.clone(),
            whitelist_manager: env_container.whitelist_manager.clone(),

            registar: Registar::default(),
            server,
        }
    }

    #[allow(dead_code)]
    pub async fn start(self) -> Environment<Running> {
        Environment {
            http_tracker_container: self.http_tracker_container.clone(),

            database: self.database.clone(),
            in_memory_torrent_repository: self.in_memory_torrent_repository.clone(),
            keys_handler: self.keys_handler.clone(),
            http_stats_repository: self.http_stats_repository.clone(),
            whitelist_manager: self.whitelist_manager.clone(),

            registar: self.registar.clone(),
            server: self
                .server
                .start(self.http_tracker_container, self.registar.give_form())
                .await
                .unwrap(),
        }
    }
}

impl Environment<Running> {
    pub async fn new(configuration: &Arc<Configuration>) -> Self {
        Environment::<Stopped>::new(configuration).start().await
    }

    pub async fn stop(self) -> Environment<Stopped> {
        Environment {
            http_tracker_container: self.http_tracker_container,

            database: self.database,
            in_memory_torrent_repository: self.in_memory_torrent_repository,
            keys_handler: self.keys_handler,
            http_stats_repository: self.http_stats_repository,
            whitelist_manager: self.whitelist_manager,

            registar: Registar::default(),
            server: self.server.stop().await.unwrap(),
        }
    }

    pub fn bind_address(&self) -> &std::net::SocketAddr {
        &self.server.state.binding
    }
}

pub struct EnvContainer {
    pub core_config: Arc<Core>,
    pub http_tracker_config: Arc<HttpTracker>,
    pub http_tracker_container: Arc<HttpTrackerContainer>,

    pub database: Arc<Box<dyn Database>>,
    pub in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
    pub keys_handler: Arc<KeysHandler>,
    pub http_stats_repository: Arc<bittorrent_http_tracker_core::statistics::repository::Repository>,
    pub whitelist_manager: Arc<WhitelistManager>,
}

impl EnvContainer {
    pub fn initialize(configuration: &Configuration) -> Self {
        let core_config = Arc::new(configuration.core.clone());
        let http_tracker_config = configuration
            .http_trackers
            .clone()
            .expect("missing HTTP tracker configuration");
        let http_tracker_config = Arc::new(http_tracker_config[0].clone());

        // HTTP stats
        let (http_stats_event_sender, http_stats_repository) =
            bittorrent_http_tracker_core::statistics::setup::factory(configuration.core.tracker_usage_statistics);
        let http_stats_event_sender = Arc::new(http_stats_event_sender);
        let http_stats_repository = Arc::new(http_stats_repository);

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

        let announce_handler = Arc::new(AnnounceHandler::new(
            &configuration.core,
            &whitelist_authorization,
            &in_memory_torrent_repository,
            &db_torrent_repository,
        ));

        let scrape_handler = Arc::new(ScrapeHandler::new(&whitelist_authorization, &in_memory_torrent_repository));

        let http_tracker_container = Arc::new(HttpTrackerContainer {
            http_tracker_config: http_tracker_config.clone(),
            core_config: core_config.clone(),
            announce_handler: announce_handler.clone(),
            scrape_handler: scrape_handler.clone(),
            whitelist_authorization: whitelist_authorization.clone(),
            http_stats_event_sender: http_stats_event_sender.clone(),
            authentication_service: authentication_service.clone(),
        });

        Self {
            core_config,
            http_tracker_config,
            http_tracker_container,

            database,
            in_memory_torrent_repository,
            keys_handler,
            http_stats_repository,
            whitelist_manager,
        }
    }
}
