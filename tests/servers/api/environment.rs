use std::net::SocketAddr;
use std::sync::Arc;

use bittorrent_primitives::info_hash::InfoHash;
use bittorrent_tracker_core::authentication::handler::KeysHandler;
use bittorrent_tracker_core::authentication::key::repository::in_memory::InMemoryKeyRepository;
use bittorrent_tracker_core::authentication::key::repository::persisted::DatabaseKeyRepository;
use bittorrent_tracker_core::authentication::service::AuthenticationService;
use bittorrent_tracker_core::databases::setup::initialize_database;
use bittorrent_tracker_core::databases::Database;
use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;
use bittorrent_tracker_core::whitelist::repository::in_memory::InMemoryWhitelist;
use bittorrent_tracker_core::whitelist::setup::initialize_whitelist_manager;
use bittorrent_udp_tracker_core::services::banning::BanService;
use bittorrent_udp_tracker_core::MAX_CONNECTION_ID_ERRORS_PER_IP;
use futures::executor::block_on;
use tokio::sync::RwLock;
use torrust_axum_server::tsl::make_rust_tls;
use torrust_server_lib::registar::Registar;
use torrust_tracker_api_client::connection_info::{ConnectionInfo, Origin};
use torrust_tracker_configuration::{Configuration, HttpApi};
use torrust_tracker_lib::bootstrap::app::initialize_global_services;
use torrust_tracker_lib::container::HttpApiContainer;
use torrust_tracker_lib::servers::apis::server::{ApiServer, Launcher, Running, Stopped};
use torrust_tracker_primitives::peer;

pub struct Environment<S>
where
    S: std::fmt::Debug + std::fmt::Display,
{
    pub http_api_container: Arc<HttpApiContainer>,

    pub database: Arc<Box<dyn Database>>,
    pub authentication_service: Arc<AuthenticationService>,
    pub in_memory_whitelist: Arc<InMemoryWhitelist>,

    pub registar: Registar,
    pub server: ApiServer<S>,
}

impl<S> Environment<S>
where
    S: std::fmt::Debug + std::fmt::Display,
{
    /// Add a torrent to the tracker
    pub fn add_torrent_peer(&self, info_hash: &InfoHash, peer: &peer::Peer) {
        let () = self
            .http_api_container
            .in_memory_torrent_repository
            .upsert_peer(info_hash, peer);
    }
}

impl Environment<Stopped> {
    pub fn new(configuration: &Arc<Configuration>) -> Self {
        initialize_global_services(configuration);

        let env_container = EnvContainer::initialize(configuration);

        let bind_to = env_container.http_api_config.bind_address;

        let tls = block_on(make_rust_tls(&env_container.http_api_config.tsl_config)).map(|tls| tls.expect("tls config failed"));

        let server = ApiServer::new(Launcher::new(bind_to, tls));

        Self {
            http_api_container: env_container.http_api_container,

            database: env_container.database.clone(),
            authentication_service: env_container.authentication_service.clone(),
            in_memory_whitelist: env_container.in_memory_whitelist.clone(),

            registar: Registar::default(),
            server,
        }
    }

    pub async fn start(self) -> Environment<Running> {
        let access_tokens = Arc::new(self.http_api_container.http_api_config.access_tokens.clone());

        Environment {
            http_api_container: self.http_api_container.clone(),

            database: self.database.clone(),
            authentication_service: self.authentication_service.clone(),
            in_memory_whitelist: self.in_memory_whitelist.clone(),

            registar: self.registar.clone(),
            server: self
                .server
                .start(self.http_api_container, self.registar.give_form(), access_tokens)
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
            http_api_container: self.http_api_container,

            database: self.database,
            authentication_service: self.authentication_service,
            in_memory_whitelist: self.in_memory_whitelist,

            registar: Registar::default(),
            server: self.server.stop().await.unwrap(),
        }
    }

    pub fn get_connection_info(&self) -> ConnectionInfo {
        let origin = Origin::new(&format!("http://{}/", self.server.state.local_addr)).unwrap(); // DevSkim: ignore DS137138

        ConnectionInfo {
            origin,
            api_token: self.http_api_container.http_api_config.access_tokens.get("admin").cloned(),
        }
    }

    pub fn bind_address(&self) -> SocketAddr {
        self.server.state.local_addr
    }
}

pub struct EnvContainer {
    pub http_api_config: Arc<HttpApi>,
    pub http_api_container: Arc<HttpApiContainer>,
    pub database: Arc<Box<dyn Database>>,
    pub authentication_service: Arc<AuthenticationService>,
    pub in_memory_whitelist: Arc<InMemoryWhitelist>,
}

impl EnvContainer {
    pub fn initialize(configuration: &Configuration) -> Self {
        let core_config = Arc::new(configuration.core.clone());
        let http_api_config = Arc::new(
            configuration
                .http_api
                .clone()
                .expect("missing HTTP API configuration")
                .clone(),
        );

        // HTTP stats
        let (_http_stats_event_sender, http_stats_repository) =
            bittorrent_http_tracker_core::statistics::setup::factory(configuration.core.tracker_usage_statistics);
        let http_stats_repository = Arc::new(http_stats_repository);

        // UDP stats
        let (_udp_stats_event_sender, udp_stats_repository) =
            bittorrent_udp_tracker_core::statistics::setup::factory(configuration.core.tracker_usage_statistics);
        let udp_stats_repository = Arc::new(udp_stats_repository);

        let ban_service = Arc::new(RwLock::new(BanService::new(MAX_CONNECTION_ID_ERRORS_PER_IP)));
        let database = initialize_database(&configuration.core);
        let in_memory_whitelist = Arc::new(InMemoryWhitelist::default());

        let whitelist_manager = initialize_whitelist_manager(database.clone(), in_memory_whitelist.clone());
        let db_key_repository = Arc::new(DatabaseKeyRepository::new(&database));
        let in_memory_key_repository = Arc::new(InMemoryKeyRepository::default());
        let authentication_service = Arc::new(AuthenticationService::new(&configuration.core, &in_memory_key_repository));
        let keys_handler = Arc::new(KeysHandler::new(
            &db_key_repository.clone(),
            &in_memory_key_repository.clone(),
        ));
        let in_memory_torrent_repository = Arc::new(InMemoryTorrentRepository::default());

        let http_api_container = Arc::new(HttpApiContainer {
            http_api_config: http_api_config.clone(),
            core_config: core_config.clone(),
            in_memory_torrent_repository: in_memory_torrent_repository.clone(),
            keys_handler: keys_handler.clone(),
            whitelist_manager: whitelist_manager.clone(),
            ban_service: ban_service.clone(),
            http_stats_repository: http_stats_repository.clone(),
            udp_stats_repository: udp_stats_repository.clone(),
        });

        Self {
            http_api_config,
            http_api_container,
            database,
            authentication_service,
            in_memory_whitelist,
        }
    }
}
