//! The `announce` service.
//!
//! The service is responsible for handling the `announce` requests.
//!
//! It delegates the `announce` logic to the [`AnnounceHandler`] and it returns
//! the [`AnnounceData`].
//!
//! It also sends an [`udp_tracker_core::statistics::event::Event`]
//! because events are specific for the HTTP tracker.
use std::net::{IpAddr, SocketAddr};
use std::ops::Range;
use std::sync::Arc;

use aquatic_udp_protocol::AnnounceRequest;
use bittorrent_primitives::info_hash::InfoHash;
use bittorrent_tracker_core::announce_handler::{AnnounceHandler, PeersWanted};
use bittorrent_tracker_core::error::{AnnounceError, WhitelistError};
use bittorrent_tracker_core::whitelist;
use bittorrent_udp_protocol::peer_builder;
use torrust_tracker_primitives::core::AnnounceData;
use torrust_tracker_primitives::peer;

use crate::packages::udp_tracker_core::connection_cookie::{check, gen_remote_fingerprint, ConnectionCookieError};
use crate::packages::udp_tracker_core::{self};

/// Errors related to announce requests.
#[derive(thiserror::Error, Debug, Clone)]
pub enum UdpAnnounceError {
    /// Error returned when there was an error with the connection cookie.
    #[error("Connection cookie error: {source}")]
    ConnectionCookieError { source: ConnectionCookieError },

    /// Error returned when there was an error with the tracker core announce handler.
    #[error("Tracker core announce error: {source}")]
    TrackerCoreAnnounceError { source: AnnounceError },

    /// Error returned when there was an error with the tracker core whitelist.
    #[error("Tracker core whitelist error: {source}")]
    TrackerCoreWhitelistError { source: WhitelistError },
}

impl From<ConnectionCookieError> for UdpAnnounceError {
    fn from(connection_cookie_error: ConnectionCookieError) -> Self {
        Self::ConnectionCookieError {
            source: connection_cookie_error,
        }
    }
}

impl From<AnnounceError> for UdpAnnounceError {
    fn from(announce_error: AnnounceError) -> Self {
        Self::TrackerCoreAnnounceError { source: announce_error }
    }
}

impl From<WhitelistError> for UdpAnnounceError {
    fn from(whitelist_error: WhitelistError) -> Self {
        Self::TrackerCoreWhitelistError { source: whitelist_error }
    }
}

/// It handles the `Announce` request.
///
/// # Errors
///
/// It will return an error if:
///
/// - The tracker is running in listed mode and the torrent is not in the
///   whitelist.
#[allow(clippy::too_many_arguments)]
pub async fn handle_announce(
    remote_addr: SocketAddr,
    request: &AnnounceRequest,
    announce_handler: &Arc<AnnounceHandler>,
    whitelist_authorization: &Arc<whitelist::authorization::WhitelistAuthorization>,
    opt_udp_stats_event_sender: &Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>>,
    cookie_valid_range: Range<f64>,
) -> Result<AnnounceData, UdpAnnounceError> {
    // todo: return a UDP response like the HTTP tracker instead of raw AnnounceData.

    // Authentication
    check(
        &request.connection_id,
        gen_remote_fingerprint(&remote_addr),
        cookie_valid_range,
    )?;

    let info_hash = request.info_hash.into();
    let remote_client_ip = remote_addr.ip();

    // Authorization
    whitelist_authorization.authorize(&info_hash).await?;

    let mut peer = peer_builder::from_request(request, &remote_client_ip);
    let peers_wanted: PeersWanted = i32::from(request.peers_wanted.0).into();

    let original_peer_ip = peer.peer_addr.ip();

    // The tracker could change the original peer ip
    let announce_data = announce_handler
        .announce(&info_hash, &mut peer, &original_peer_ip, &peers_wanted)
        .await?;

    if let Some(udp_stats_event_sender) = opt_udp_stats_event_sender.as_deref() {
        match original_peer_ip {
            IpAddr::V4(_) => {
                udp_stats_event_sender
                    .send_event(udp_tracker_core::statistics::event::Event::Udp4Announce)
                    .await;
            }
            IpAddr::V6(_) => {
                udp_stats_event_sender
                    .send_event(udp_tracker_core::statistics::event::Event::Udp6Announce)
                    .await;
            }
        }
    }

    Ok(announce_data)
}

/// # Errors
///
/// It will return an error if the announce request fails.
pub async fn invoke(
    announce_handler: Arc<AnnounceHandler>,
    opt_udp_stats_event_sender: Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>>,
    info_hash: InfoHash,
    peer: &mut peer::Peer,
    peers_wanted: &PeersWanted,
) -> Result<AnnounceData, AnnounceError> {
    let original_peer_ip = peer.peer_addr.ip();

    // The tracker could change the original peer ip
    let announce_data = announce_handler
        .announce(&info_hash, peer, &original_peer_ip, peers_wanted)
        .await?;

    if let Some(udp_stats_event_sender) = opt_udp_stats_event_sender.as_deref() {
        match original_peer_ip {
            IpAddr::V4(_) => {
                udp_stats_event_sender
                    .send_event(udp_tracker_core::statistics::event::Event::Udp4Announce)
                    .await;
            }
            IpAddr::V6(_) => {
                udp_stats_event_sender
                    .send_event(udp_tracker_core::statistics::event::Event::Udp6Announce)
                    .await;
            }
        }
    }

    Ok(announce_data)
}
