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
use std::sync::Arc;

use aquatic_udp_protocol::AnnounceRequest;
use bittorrent_primitives::info_hash::InfoHash;
use bittorrent_tracker_core::announce_handler::{AnnounceHandler, PeersWanted};
use bittorrent_tracker_core::error::AnnounceError;
use bittorrent_tracker_core::whitelist;
use torrust_tracker_primitives::core::AnnounceData;
use torrust_tracker_primitives::peer;

use crate::packages::udp_tracker_core::{self, peer_builder};

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
) -> Result<AnnounceData, AnnounceError> {
    let info_hash = request.info_hash.into();
    let remote_client_ip = remote_addr.ip();

    // Authorization
    whitelist_authorization.authorize(&info_hash).await?;

    let mut peer = peer_builder::from_request(request, &remote_client_ip);
    let peers_wanted: PeersWanted = i32::from(request.peers_wanted.0).into();

    let announce_data = invoke(
        announce_handler.clone(),
        opt_udp_stats_event_sender.clone(),
        info_hash,
        &mut peer,
        &peers_wanted,
    )
    .await?;

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
