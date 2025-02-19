//! Logic to run the HTTP API server.
//!
//! It contains two main structs: `ApiServer` and `Launcher`,
//! and two main functions: `start` and `start_tls`.
//!
//! The `ApiServer` struct is responsible for:
//! - Starting and stopping the server.
//! - Storing the configuration.
//!
//! `ApiServer` relies on a launcher to start the actual server.
///
/// 1. `ApiServer::start` -> spawns new asynchronous task.
/// 2. `Launcher::start` -> starts the server on the spawned task.
///
/// The `Launcher` struct is responsible for:
///
/// - Knowing how to start the server with graceful shutdown.
///
/// For the time being the `ApiServer` and `Launcher` are only used in tests
/// where we need to start and stop the server multiple times. In production
/// code and the main application uses the `start` and `start_tls` functions
/// to start the servers directly since we do not need to control the server
/// when it's running. In the future we might need to control the server,
/// for example, to restart it to apply new configuration changes, to remotely
/// shutdown the server, etc.
use std::net::SocketAddr;
use std::sync::Arc;

use axum_server::tls_rustls::RustlsConfig;
use axum_server::Handle;
use derive_more::derive::Display;
use derive_more::Constructor;
use futures::future::BoxFuture;
use thiserror::Error;
use tokio::sync::oneshot::{Receiver, Sender};
use torrust_axum_server::custom_axum_server::{self, TimeoutAcceptor};
use torrust_axum_server::signals::graceful_shutdown;
use torrust_server_lib::logging::STARTED_ON;
use torrust_server_lib::registar::{ServiceHealthCheckJob, ServiceRegistration, ServiceRegistrationForm};
use torrust_server_lib::signals::{Halted, Started};
use torrust_tracker_configuration::AccessTokens;
use tracing::{instrument, Level};

use super::routes::router;
use crate::container::HttpApiContainer;
use crate::servers::apis::API_LOG_TARGET;

/// Errors that can occur when starting or stopping the API server.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Error when starting or stopping the API server")]
    FailedToStartOrStop(String),
}

/// An alias for the `ApiServer` struct with the `Stopped` state.
#[allow(clippy::module_name_repetitions)]
pub type StoppedApiServer = ApiServer<Stopped>;

/// An alias for the `ApiServer` struct with the `Running` state.
#[allow(clippy::module_name_repetitions)]
pub type RunningApiServer = ApiServer<Running>;

/// A struct responsible for starting and stopping an API server with a
/// specific configuration and keeping track of the started server.
///
/// It's a state machine that can be in one of two
/// states: `Stopped` or `Running`.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Display)]
pub struct ApiServer<S>
where
    S: std::fmt::Debug + std::fmt::Display,
{
    pub state: S,
}

/// The `Stopped` state of the `ApiServer` struct.
#[derive(Debug, Display)]
#[display("Stopped: {launcher}")]
pub struct Stopped {
    launcher: Launcher,
}

/// The `Running` state of the `ApiServer` struct.
#[derive(Debug, Display)]
#[display("Running (with local address): {local_addr}")]
pub struct Running {
    pub local_addr: SocketAddr,
    pub halt_task: tokio::sync::oneshot::Sender<Halted>,
    pub task: tokio::task::JoinHandle<Launcher>,
}

impl Running {
    #[must_use]
    pub fn new(
        local_addr: SocketAddr,
        halt_task: tokio::sync::oneshot::Sender<Halted>,
        task: tokio::task::JoinHandle<Launcher>,
    ) -> Self {
        Self {
            local_addr,
            halt_task,
            task,
        }
    }
}

impl ApiServer<Stopped> {
    #[must_use]
    pub fn new(launcher: Launcher) -> Self {
        Self {
            state: Stopped { launcher },
        }
    }

    /// Starts the API server with the given configuration.
    ///
    /// # Errors
    ///
    /// It would return an error if no `SocketAddr` is returned after launching the server.
    ///
    /// # Panics
    ///
    /// It would panic if the bound socket address cannot be sent back to this starter.
    #[instrument(skip(self, http_api_container, form, access_tokens), err, ret(Display, level = Level::INFO))]
    pub async fn start(
        self,
        http_api_container: Arc<HttpApiContainer>,
        form: ServiceRegistrationForm,
        access_tokens: Arc<AccessTokens>,
    ) -> Result<ApiServer<Running>, Error> {
        let (tx_start, rx_start) = tokio::sync::oneshot::channel::<Started>();
        let (tx_halt, rx_halt) = tokio::sync::oneshot::channel::<Halted>();

        let launcher = self.state.launcher;

        let task = tokio::spawn(async move {
            tracing::debug!(target: API_LOG_TARGET, "Starting with launcher in spawned task ...");

            let _task = launcher.start(http_api_container, access_tokens, tx_start, rx_halt).await;

            tracing::debug!(target: API_LOG_TARGET, "Started with launcher in spawned task");

            launcher
        });

        let api_server = match rx_start.await {
            Ok(started) => {
                form.send(ServiceRegistration::new(started.address, check_fn))
                    .expect("it should be able to send service registration");

                ApiServer {
                    state: Running::new(started.address, tx_halt, task),
                }
            }
            Err(err) => {
                let msg = format!("Unable to start API server: {err}");
                tracing::error!("{}", msg);
                panic!("{}", msg);
            }
        };

        Ok(api_server)
    }
}

impl ApiServer<Running> {
    /// Stops the API server.
    ///
    /// # Errors
    ///
    /// It would return an error if the channel for the task killer signal was closed.
    #[instrument(skip(self), err, ret(Display, level = Level::INFO))]
    pub async fn stop(self) -> Result<ApiServer<Stopped>, Error> {
        self.state
            .halt_task
            .send(Halted::Normal)
            .map_err(|_| Error::FailedToStartOrStop("Task killer channel was closed.".to_string()))?;

        let launcher = self.state.task.await.map_err(|e| Error::FailedToStartOrStop(e.to_string()))?;

        Ok(ApiServer {
            state: Stopped { launcher },
        })
    }
}

/// Checks the Health by connecting to the API service endpoint.
///
/// # Errors
///
/// This function will return an error if unable to connect.
/// Or if there request returns an error code.
#[must_use]
#[instrument(skip())]
pub fn check_fn(binding: &SocketAddr) -> ServiceHealthCheckJob {
    let url = format!("http://{binding}/api/health_check"); // DevSkim: ignore DS137138

    let info = format!("checking api health check at: {url}");

    let job = tokio::spawn(async move {
        match reqwest::get(url).await {
            Ok(response) => Ok(response.status().to_string()),
            Err(err) => Err(err.to_string()),
        }
    });
    ServiceHealthCheckJob::new(*binding, info, job)
}

/// A struct responsible for starting the API server.
#[derive(Constructor, Debug)]
pub struct Launcher {
    bind_to: SocketAddr,
    tls: Option<RustlsConfig>,
}

impl std::fmt::Display for Launcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.tls.is_some() {
            write!(f, "(with socket): {}, using TLS", self.bind_to,)
        } else {
            write!(f, "(with socket): {}, without TLS", self.bind_to,)
        }
    }
}

impl Launcher {
    /// Starts the API server with graceful shutdown.
    ///
    /// If TLS is enabled in the configuration, it will start the server with
    /// TLS. See [`torrust-tracker-configuration`](torrust_tracker_configuration)
    /// for more  information about configuration.
    ///
    /// # Panics
    ///
    /// Will panic if unable to bind to the socket, or unable to get the address of the bound socket.
    /// Will also panic if unable to send message regarding the bound socket address.
    #[instrument(skip(self, http_api_container, access_tokens, tx_start, rx_halt))]
    pub fn start(
        &self,
        http_api_container: Arc<HttpApiContainer>,
        access_tokens: Arc<AccessTokens>,
        tx_start: Sender<Started>,
        rx_halt: Receiver<Halted>,
    ) -> BoxFuture<'static, ()> {
        let socket = std::net::TcpListener::bind(self.bind_to).expect("Could not bind tcp_listener to address.");
        let address = socket.local_addr().expect("Could not get local_addr from tcp_listener.");

        let router = router(http_api_container, access_tokens, address);

        let handle = Handle::new();

        tokio::task::spawn(graceful_shutdown(
            handle.clone(),
            rx_halt,
            format!("Shutting down tracker API server on socket address: {address}"),
        ));

        let tls = self.tls.clone();
        let protocol = if tls.is_some() { "https" } else { "http" };

        tracing::info!(target: API_LOG_TARGET, "Starting on {protocol}://{}", address);

        let running = Box::pin(async {
            match tls {
                Some(tls) => custom_axum_server::from_tcp_rustls_with_timeouts(socket, tls)
                    .handle(handle)
                    // The TimeoutAcceptor is commented because TSL does not work with it.
                    // See: https://github.com/torrust/torrust-index/issues/204#issuecomment-2115529214
                    //.acceptor(TimeoutAcceptor)
                    .serve(router.into_make_service_with_connect_info::<std::net::SocketAddr>())
                    .await
                    .expect("Axum server for tracker API crashed."),
                None => custom_axum_server::from_tcp_with_timeouts(socket)
                    .handle(handle)
                    .acceptor(TimeoutAcceptor)
                    .serve(router.into_make_service_with_connect_info::<std::net::SocketAddr>())
                    .await
                    .expect("Axum server for tracker API crashed."),
            }
        });

        tracing::info!(target: API_LOG_TARGET, "{STARTED_ON} {protocol}://{}", address);

        tx_start
            .send(Started { address })
            .expect("the HTTP(s) Tracker API service should not be dropped");

        running
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use torrust_axum_server::tsl::make_rust_tls;
    use torrust_server_lib::registar::Registar;
    use torrust_tracker_test_helpers::configuration::ephemeral_public;

    use crate::bootstrap::app::{initialize_app_container, initialize_global_services};
    use crate::container::HttpApiContainer;
    use crate::servers::apis::server::{ApiServer, Launcher};

    #[tokio::test]
    async fn it_should_be_able_to_start_and_stop() {
        let cfg = Arc::new(ephemeral_public());
        let http_api_config = Arc::new(cfg.http_api.clone().unwrap());

        initialize_global_services(&cfg);

        let app_container = Arc::new(initialize_app_container(&cfg));

        let bind_to = http_api_config.bind_address;

        let tls = make_rust_tls(&http_api_config.tsl_config)
            .await
            .map(|tls| tls.expect("tls config failed"));

        let access_tokens = Arc::new(http_api_config.access_tokens.clone());

        let stopped = ApiServer::new(Launcher::new(bind_to, tls));

        let register = &Registar::default();

        let http_api_container = Arc::new(HttpApiContainer::from_app_container(&http_api_config, &app_container));

        let started = stopped
            .start(http_api_container, register.give_form(), access_tokens)
            .await
            .expect("it should start the server");
        let stopped = started.stop().await.expect("it should stop the server");

        assert_eq!(stopped.state.launcher.bind_to, bind_to);
    }
}
