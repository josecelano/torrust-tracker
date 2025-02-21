use std::net::SocketAddr;
use std::sync::Arc;

use bittorrent_primitives::info_hash::InfoHash;
use bittorrent_tracker_core::announce_handler::AnnounceHandler;
use bittorrent_tracker_core::databases::setup::initialize_database;
use bittorrent_tracker_core::databases::Database;
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;
use bittorrent_tracker_core::torrent::repository::persisted::DatabasePersistentTorrentRepository;
use bittorrent_tracker_core::whitelist::authorization::WhitelistAuthorization;
use bittorrent_tracker_core::whitelist::repository::in_memory::InMemoryWhitelist;
use bittorrent_udp_tracker_core::container::UdpTrackerCoreContainer;
use bittorrent_udp_tracker_core::services::banning::BanService;
use bittorrent_udp_tracker_core::{statistics, MAX_CONNECTION_ID_ERRORS_PER_IP};
use tokio::sync::RwLock;
use torrust_server_lib::registar::Registar;
use torrust_tracker_configuration::{Configuration, Core, UdpTracker, DEFAULT_TIMEOUT};
use torrust_tracker_lib::bootstrap::app::initialize_global_services;
use torrust_tracker_lib::servers::udp::server::spawner::Spawner;
use torrust_tracker_lib::servers::udp::server::states::{Running, Stopped};
use torrust_tracker_lib::servers::udp::server::Server;
use torrust_tracker_primitives::peer;

pub struct Environment<S>
where
    S: std::fmt::Debug + std::fmt::Display,
{
    pub udp_tracker_container: Arc<UdpTrackerCoreContainer>,

    pub database: Arc<Box<dyn Database>>,
    pub in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
    pub udp_stats_repository: Arc<statistics::repository::Repository>,

    pub registar: Registar,
    pub server: Server<S>,
}

impl<S> Environment<S>
where
    S: std::fmt::Debug + std::fmt::Display,
{
    /// Add a torrent to the tracker
    #[allow(dead_code)]
    pub fn add_torrent(&self, info_hash: &InfoHash, peer: &peer::Peer) {
        let () = self.in_memory_torrent_repository.upsert_peer(info_hash, peer);
    }
}

impl Environment<Stopped> {
    #[allow(dead_code)]
    pub fn new(configuration: &Arc<Configuration>) -> Self {
        initialize_global_services(configuration);

        let env_container = EnvContainer::initialize(configuration);

        let bind_to = env_container.udp_tracker_config.bind_address;

        let server = Server::new(Spawner::new(bind_to));

        let udp_tracker_container = Arc::new(UdpTrackerCoreContainer {
            core_config: env_container.core_config.clone(),
            announce_handler: env_container.udp_tracker_core_container.announce_handler.clone(),
            scrape_handler: env_container.udp_tracker_core_container.scrape_handler.clone(),
            whitelist_authorization: env_container.udp_tracker_core_container.whitelist_authorization.clone(),

            udp_tracker_config: env_container.udp_tracker_config.clone(),
            udp_stats_event_sender: env_container.udp_tracker_core_container.udp_stats_event_sender.clone(),
            udp_stats_repository: env_container.udp_tracker_core_container.udp_stats_repository.clone(),
            ban_service: env_container.udp_tracker_core_container.ban_service.clone(),
        });

        Self {
            udp_tracker_container,

            database: env_container.database.clone(),
            in_memory_torrent_repository: env_container.in_memory_torrent_repository.clone(),
            udp_stats_repository: env_container.udp_stats_repository.clone(),

            registar: Registar::default(),
            server,
        }
    }

    #[allow(dead_code)]
    pub async fn start(self) -> Environment<Running> {
        let cookie_lifetime = self.udp_tracker_container.udp_tracker_config.cookie_lifetime;

        Environment {
            udp_tracker_container: self.udp_tracker_container.clone(),

            database: self.database.clone(),
            in_memory_torrent_repository: self.in_memory_torrent_repository.clone(),
            udp_stats_repository: self.udp_stats_repository.clone(),

            registar: self.registar.clone(),
            server: self
                .server
                .start(self.udp_tracker_container, self.registar.give_form(), cookie_lifetime)
                .await
                .unwrap(),
        }
    }
}

impl Environment<Running> {
    pub async fn new(configuration: &Arc<Configuration>) -> Self {
        tokio::time::timeout(DEFAULT_TIMEOUT, Environment::<Stopped>::new(configuration).start())
            .await
            .expect("it should create an environment within the timeout")
    }

    #[allow(dead_code)]
    pub async fn stop(self) -> Environment<Stopped> {
        let stopped = tokio::time::timeout(DEFAULT_TIMEOUT, self.server.stop())
            .await
            .expect("it should stop the environment within the timeout");

        Environment {
            udp_tracker_container: self.udp_tracker_container,

            database: self.database,
            in_memory_torrent_repository: self.in_memory_torrent_repository,
            udp_stats_repository: self.udp_stats_repository,

            registar: Registar::default(),
            server: stopped.expect("it stop the udp tracker service"),
        }
    }

    pub fn bind_address(&self) -> SocketAddr {
        self.server.state.local_addr
    }
}

pub struct EnvContainer {
    pub core_config: Arc<Core>,
    pub udp_tracker_config: Arc<UdpTracker>,
    pub udp_tracker_core_container: Arc<UdpTrackerCoreContainer>,

    pub database: Arc<Box<dyn Database>>,
    pub in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
    pub udp_stats_repository: Arc<bittorrent_udp_tracker_core::statistics::repository::Repository>,
}

impl EnvContainer {
    pub fn initialize(configuration: &Configuration) -> Self {
        let core_config = Arc::new(configuration.core.clone());
        let udp_tracker_configurations = configuration.udp_trackers.clone().expect("missing UDP tracker configuration");
        let udp_tracker_config = Arc::new(udp_tracker_configurations[0].clone());

        // UDP stats
        let (udp_stats_event_sender, udp_stats_repository) =
            bittorrent_udp_tracker_core::statistics::setup::factory(configuration.core.tracker_usage_statistics);
        let udp_stats_event_sender = Arc::new(udp_stats_event_sender);
        let udp_stats_repository = Arc::new(udp_stats_repository);

        let ban_service = Arc::new(RwLock::new(BanService::new(MAX_CONNECTION_ID_ERRORS_PER_IP)));
        let database = initialize_database(&configuration.core);
        let in_memory_whitelist = Arc::new(InMemoryWhitelist::default());
        let whitelist_authorization = Arc::new(WhitelistAuthorization::new(&configuration.core, &in_memory_whitelist.clone()));
        let in_memory_torrent_repository = Arc::new(InMemoryTorrentRepository::default());
        let db_torrent_repository = Arc::new(DatabasePersistentTorrentRepository::new(&database));

        let announce_handler = Arc::new(AnnounceHandler::new(
            &configuration.core,
            &whitelist_authorization,
            &in_memory_torrent_repository,
            &db_torrent_repository,
        ));

        let scrape_handler = Arc::new(ScrapeHandler::new(&whitelist_authorization, &in_memory_torrent_repository));

        let udp_tracker_container = Arc::new(UdpTrackerCoreContainer {
            core_config: core_config.clone(),
            announce_handler: announce_handler.clone(),
            scrape_handler: scrape_handler.clone(),
            whitelist_authorization: whitelist_authorization.clone(),

            udp_tracker_config: udp_tracker_config.clone(),
            udp_stats_event_sender: udp_stats_event_sender.clone(),
            udp_stats_repository: udp_stats_repository.clone(),
            ban_service: ban_service.clone(),
        });

        Self {
            core_config,
            udp_tracker_config,
            udp_tracker_core_container: udp_tracker_container,

            database,
            in_memory_torrent_repository,
            udp_stats_repository,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::sleep;
    use torrust_tracker_test_helpers::configuration;

    use crate::common::logging;
    use crate::servers::udp::Started;

    #[tokio::test]
    async fn it_should_make_and_stop_udp_server() {
        logging::setup();

        let env = Started::new(&configuration::ephemeral().into()).await;
        sleep(Duration::from_secs(1)).await;
        env.stop().await;
        sleep(Duration::from_secs(1)).await;
    }
}
