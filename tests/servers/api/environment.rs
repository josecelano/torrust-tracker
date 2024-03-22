use std::net::SocketAddr;
use std::sync::Arc;

use futures::executor::block_on;
use torrust_tracker::bootstrap::app::initialize_with_configuration;
use torrust_tracker::bootstrap::jobs::make_rust_tls;
use torrust_tracker::core::peer::Peer;
use torrust_tracker::core::Tracker;
use torrust_tracker::servers::apis::server::{ApiServer, Launcher, Running, Stopped};
use torrust_tracker::servers::registar::Registar;
use torrust_tracker::shared::bit_torrent::info_hash::InfoHash;
use torrust_tracker_configuration::{Configuration, HttpApi};

use super::connection_info::ConnectionInfo;

pub struct Environment<S> {
    pub config: Arc<HttpApi>,
    pub tracker: Arc<Tracker>,
    pub registar: Registar,
    pub server: ApiServer<S>,
}

impl<S> Environment<S> {
    /// Add a torrent to the tracker
    pub async fn add_torrent_peer(&self, info_hash: &InfoHash, peer: &Peer) {
        self.tracker.inner_announce(info_hash, peer).await;
    }
}

impl Environment<Stopped> {
    pub fn new(configuration: &Arc<Configuration>) -> Self {
        let tracker = initialize_with_configuration(configuration);

        let config = Arc::new(configuration.http_api.clone());

        let bind_to = config
            .bind_address
            .parse::<std::net::SocketAddr>()
            .expect("Tracker API bind_address invalid.");

        let tls = block_on(make_rust_tls(config.ssl_enabled, &config.ssl_cert_path, &config.ssl_key_path))
            .map(|tls| tls.expect("tls config failed"));

        let server = ApiServer::new(Launcher::new(bind_to, tls));

        Self {
            config,
            tracker,
            registar: Registar::default(),
            server,
        }
    }

    pub async fn start(self) -> Environment<Running> {
        let access_tokens = Arc::new(self.config.access_tokens.clone());

        Environment {
            config: self.config,
            tracker: self.tracker.clone(),
            registar: self.registar.clone(),
            server: self
                .server
                .start(self.tracker, self.registar.give_form(), access_tokens)
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
            config: self.config,
            tracker: self.tracker,
            registar: Registar::default(),
            server: self.server.stop().await.unwrap(),
        }
    }

    pub fn get_connection_info(&self) -> ConnectionInfo {
        ConnectionInfo {
            bind_address: self.server.state.binding.to_string(),
            api_token: self.config.access_tokens.get("admin").cloned(),
        }
    }

    pub fn bind_address(&self) -> SocketAddr {
        self.server.state.binding
    }
}
