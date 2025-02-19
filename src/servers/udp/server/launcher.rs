use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bittorrent_tracker_client::udp::client::check;
use bittorrent_udp_tracker_core::{self, statistics, UDP_TRACKER_LOG_TARGET};
use derive_more::Constructor;
use futures_util::StreamExt;
use tokio::select;
use tokio::sync::oneshot;
use tokio::time::interval;
use torrust_server_lib::logging::STARTED_ON;
use torrust_server_lib::registar::ServiceHealthCheckJob;
use torrust_server_lib::signals::{shutdown_signal_with_message, Halted};
use tracing::instrument;

use super::request_buffer::ActiveRequests;
use crate::bootstrap::jobs::Started;
use crate::container::UdpTrackerContainer;
use crate::servers::udp::server::bound_socket::BoundSocket;
use crate::servers::udp::server::processor::Processor;
use crate::servers::udp::server::receiver::Receiver;

const IP_BANS_RESET_INTERVAL_IN_SECS: u64 = 3600;

/// A UDP server instance launcher.
#[derive(Constructor)]
pub struct Launcher;

impl Launcher {
    /// It starts the UDP server instance with graceful shutdown.
    ///
    /// # Panics
    ///
    /// It panics if unable to bind to udp socket, and get the address from the udp socket.
    /// It panics if unable to send address of socket.
    /// It panics if the udp server is loaded when the tracker is private.
    #[instrument(skip(udp_tracker_container, bind_to, tx_start, rx_halt))]
    pub async fn run_with_graceful_shutdown(
        udp_tracker_container: Arc<UdpTrackerContainer>,
        bind_to: SocketAddr,
        cookie_lifetime: Duration,
        tx_start: oneshot::Sender<Started>,
        rx_halt: oneshot::Receiver<Halted>,
    ) {
        tracing::info!(target: UDP_TRACKER_LOG_TARGET, "Starting on: {bind_to}");

        if udp_tracker_container.core_config.private {
            tracing::error!("udp services cannot be used for private trackers");
            panic!("it should not use udp if using authentication");
        }

        let socket = tokio::time::timeout(Duration::from_millis(5000), BoundSocket::new(bind_to))
            .await
            .expect("it should bind to the socket within five seconds");

        let bound_socket = match socket {
            Ok(socket) => socket,
            Err(e) => {
                tracing::error!(target: UDP_TRACKER_LOG_TARGET, addr = %bind_to, err = %e, "Udp::run_with_graceful_shutdown panic! (error when building socket)" );
                panic!("could not bind to socket!");
            }
        };

        let address = bound_socket.address();
        let local_udp_url = bound_socket.url().to_string();

        tracing::info!(target: UDP_TRACKER_LOG_TARGET, "{STARTED_ON}: {local_udp_url}");

        let receiver = Receiver::new(bound_socket.into());

        tracing::trace!(target: UDP_TRACKER_LOG_TARGET, local_udp_url, "Udp::run_with_graceful_shutdown (spawning main loop)");

        let running = {
            let local_addr = local_udp_url.clone();
            tokio::task::spawn(async move {
                tracing::debug!(target: UDP_TRACKER_LOG_TARGET, local_addr, "Udp::run_with_graceful_shutdown::task (listening...)");
                let () = Self::run_udp_server_main(receiver, udp_tracker_container, cookie_lifetime).await;
            })
        };

        tx_start
            .send(Started { address })
            .expect("the UDP Tracker service should not be dropped");

        tracing::debug!(target: UDP_TRACKER_LOG_TARGET, local_udp_url, "Udp::run_with_graceful_shutdown (started)");

        let stop = running.abort_handle();

        let halt_task = tokio::task::spawn(shutdown_signal_with_message(
            rx_halt,
            format!("Halting UDP Service Bound to Socket: {address}"),
        ));

        select! {
            _ = running => { tracing::debug!(target: UDP_TRACKER_LOG_TARGET, local_udp_url, "Udp::run_with_graceful_shutdown (stopped)"); },
            _ = halt_task => { tracing::debug!(target: UDP_TRACKER_LOG_TARGET, local_udp_url, "Udp::run_with_graceful_shutdown (halting)"); }
        }
        stop.abort();

        tokio::task::yield_now().await; // lets allow the other threads to complete.
    }

    #[must_use]
    #[instrument(skip(binding))]
    pub fn check(binding: &SocketAddr) -> ServiceHealthCheckJob {
        let binding = *binding;
        let info = format!("checking the udp tracker health check at: {binding}");

        let job = tokio::spawn(async move { check(&binding).await });

        ServiceHealthCheckJob::new(binding, info, job)
    }

    #[instrument(skip(receiver, udp_tracker_container))]
    async fn run_udp_server_main(
        mut receiver: Receiver,
        udp_tracker_container: Arc<UdpTrackerContainer>,
        cookie_lifetime: Duration,
    ) {
        let active_requests = &mut ActiveRequests::default();

        let addr = receiver.bound_socket_address();

        let local_addr = format!("udp://{addr}");

        let cookie_lifetime = cookie_lifetime.as_secs_f64();

        let ban_cleaner = udp_tracker_container.ban_service.clone();

        tokio::spawn(async move {
            let mut cleaner_interval = interval(Duration::from_secs(IP_BANS_RESET_INTERVAL_IN_SECS));

            cleaner_interval.tick().await;

            loop {
                cleaner_interval.tick().await;
                ban_cleaner.write().await.reset_bans();
            }
        });

        loop {
            if let Some(req) = {
                tracing::trace!(target: UDP_TRACKER_LOG_TARGET, local_addr, "Udp::run_udp_server (wait for request)");
                receiver.next().await
            } {
                tracing::trace!(target: UDP_TRACKER_LOG_TARGET, local_addr, "Udp::run_udp_server::loop (in)");

                let req = match req {
                    Ok(req) => req,
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::Interrupted {
                            tracing::warn!(target: UDP_TRACKER_LOG_TARGET, local_addr, err = %e,  "Udp::run_udp_server::loop (interrupted)");
                            return;
                        }
                        tracing::error!(target: UDP_TRACKER_LOG_TARGET, local_addr, err = %e,  "Udp::run_udp_server::loop break: (got error)");
                        break;
                    }
                };

                if let Some(udp_stats_event_sender) = udp_tracker_container.udp_stats_event_sender.as_deref() {
                    match req.from.ip() {
                        IpAddr::V4(_) => {
                            udp_stats_event_sender.send_event(statistics::event::Event::Udp4Request).await;
                        }
                        IpAddr::V6(_) => {
                            udp_stats_event_sender.send_event(statistics::event::Event::Udp6Request).await;
                        }
                    }
                }

                if udp_tracker_container.ban_service.read().await.is_banned(&req.from.ip()) {
                    tracing::debug!(target: UDP_TRACKER_LOG_TARGET, local_addr,  "Udp::run_udp_server::loop continue: (banned ip)");

                    if let Some(udp_stats_event_sender) = udp_tracker_container.udp_stats_event_sender.as_deref() {
                        udp_stats_event_sender
                            .send_event(statistics::event::Event::UdpRequestBanned)
                            .await;
                    }

                    continue;
                }

                let processor = Processor::new(receiver.socket.clone(), udp_tracker_container.clone(), cookie_lifetime);

                /* We spawn the new task even if the active requests buffer is
                full. This could seem counterintuitive because we are accepting
                more request and consuming more memory even if the server is
                already busy. However, we "force_push" the new tasks in the
                buffer. That means, in the worst scenario we will abort a
                running task to make place for the new task.

                Once concern could be to reach an starvation point were we are
                only adding and removing tasks without given them the chance to
                finish. However, the buffer is yielding before aborting one
                tasks, giving it the chance to finish. */
                let abort_handle: tokio::task::AbortHandle = tokio::task::spawn(processor.process_request(req)).abort_handle();

                if abort_handle.is_finished() {
                    continue;
                }

                let old_request_aborted = active_requests.force_push(abort_handle, &local_addr).await;

                if old_request_aborted {
                    // Evicted task from active requests buffer was aborted.

                    if let Some(udp_stats_event_sender) = udp_tracker_container.udp_stats_event_sender.as_deref() {
                        udp_stats_event_sender
                            .send_event(statistics::event::Event::UdpRequestAborted)
                            .await;
                    }
                }
            } else {
                tokio::task::yield_now().await;

                // the request iterator returned `None`.
                tracing::error!(target: UDP_TRACKER_LOG_TARGET, local_addr, "Udp::run_udp_server breaking: (ran dry, should not happen in production!)");
                break;
            }
        }
    }
}
