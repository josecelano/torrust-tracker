use std::net::SocketAddr;
use std::sync::Arc;

use torrust_tracker_configuration::TrackerPolicy;
use torrust_tracker_primitives::announce_event::AnnounceEvent;
use torrust_tracker_primitives::peer::{self, ReadInfo};
use torrust_tracker_primitives::swarm_metadata::SwarmMetadata;
use torrust_tracker_primitives::DurationSinceUnixEpoch;

use super::Entry;
use crate::{BTreeMapPeerList, EntrySingle, SkipMapPeerList};

impl Entry for EntrySingle<BTreeMapPeerList> {
    #[allow(clippy::cast_possible_truncation)]
    fn get_swarm_metadata(&self) -> SwarmMetadata {
        let complete: u32 = self.peers.values().filter(|peer| peer.is_seeder()).count() as u32;
        let incomplete: u32 = self.peers.len() as u32 - complete;

        SwarmMetadata {
            downloaded: self.downloaded,
            complete,
            incomplete,
        }
    }

    fn is_good(&self, policy: &TrackerPolicy) -> bool {
        if policy.persistent_torrent_completed_stat && self.downloaded > 0 {
            return true;
        }

        if policy.remove_peerless_torrents && self.peers.is_empty() {
            return false;
        }

        true
    }

    fn peers_is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    fn get_peers_len(&self) -> usize {
        self.peers.len()
    }
    fn get_peers(&self, limit: Option<usize>) -> Vec<Arc<peer::Peer>> {
        match limit {
            Some(limit) => self.peers.values().take(limit).cloned().collect(),
            None => self.peers.values().cloned().collect(),
        }
    }

    fn get_peers_for_client(&self, client: &SocketAddr, limit: Option<usize>) -> Vec<Arc<peer::Peer>> {
        match limit {
            Some(limit) => self
                .peers
                .values()
                // Take peers which are not the client peer
                .filter(|peer| peer::ReadInfo::get_address(peer.as_ref()) != *client)
                // Limit the number of peers on the result
                .take(limit)
                .cloned()
                .collect(),
            None => self
                .peers
                .values()
                // Take peers which are not the client peer
                .filter(|peer| peer::ReadInfo::get_address(peer.as_ref()) != *client)
                .cloned()
                .collect(),
        }
    }

    fn upsert_peer(&mut self, peer: &peer::Peer) -> bool {
        let mut downloaded_stats_updated: bool = false;

        match peer::ReadInfo::get_event(peer) {
            AnnounceEvent::Stopped => {
                drop(self.peers.remove(&peer::ReadInfo::get_id(peer)));
            }
            AnnounceEvent::Completed => {
                let previous = self.peers.insert(peer::ReadInfo::get_id(peer), Arc::new(*peer));
                // Don't count if peer was not previously known and not already completed.
                if previous.is_some_and(|p| p.event != AnnounceEvent::Completed) {
                    self.downloaded += 1;
                    downloaded_stats_updated = true;
                }
            }
            _ => {
                drop(self.peers.insert(peer::ReadInfo::get_id(peer), Arc::new(*peer)));
            }
        }

        downloaded_stats_updated
    }

    fn remove_inactive_peers(&mut self, current_cutoff: DurationSinceUnixEpoch) {
        self.peers
            .retain(|_, peer| peer::ReadInfo::get_updated(peer) > current_cutoff);
    }
}

impl Entry for EntrySingle<SkipMapPeerList> {
    #[allow(clippy::cast_possible_truncation)]
    fn get_swarm_metadata(&self) -> SwarmMetadata {
        let complete: u32 = self.peers.iter().filter(|entry| entry.value().is_seeder()).count() as u32;
        let incomplete: u32 = self.peers.len() as u32 - complete;

        SwarmMetadata {
            downloaded: self.downloaded,
            complete,
            incomplete,
        }
    }

    fn is_good(&self, policy: &TrackerPolicy) -> bool {
        if policy.persistent_torrent_completed_stat && self.downloaded > 0 {
            return true;
        }

        if policy.remove_peerless_torrents && self.peers.is_empty() {
            return false;
        }

        true
    }

    fn peers_is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    fn get_peers_len(&self) -> usize {
        self.peers.len()
    }
    fn get_peers(&self, limit: Option<usize>) -> Vec<Arc<peer::Peer>> {
        match limit {
            Some(limit) => self.peers.iter().take(limit).map(|entry| entry.value().clone()).collect(),
            None => self.peers.iter().map(|entry| entry.value().clone()).collect(),
        }
    }

    fn get_peers_for_client(&self, client: &SocketAddr, limit: Option<usize>) -> Vec<Arc<peer::Peer>> {
        match limit {
            Some(limit) => self
                .peers
                .iter()
                // Take peers which are not the client peer
                .filter(|entry| peer::ReadInfo::get_address(entry.value().as_ref()) != *client)
                // Limit the number of peers on the result
                .take(limit)
                .map(|entry| entry.value().clone())
                .collect(),
            None => self
                .peers
                .iter()
                // Take peers which are not the client peer
                .filter(|entry| peer::ReadInfo::get_address(entry.value().as_ref()) != *client)
                .map(|entry| entry.value().clone())
                .collect(),
        }
    }

    fn upsert_peer(&mut self, peer: &peer::Peer) -> bool {
        let mut downloaded_stats_updated: bool = false;

        match peer::ReadInfo::get_event(peer) {
            AnnounceEvent::Stopped => {
                drop(self.peers.remove(&peer::ReadInfo::get_id(peer)));
            }
            AnnounceEvent::Completed => {
                let previous = self.peers.get(&peer.get_id());

                let increase_downloads = match previous {
                    Some(entry) => {
                        // Don't count if peer was already completed.
                        entry.value().event != AnnounceEvent::Completed
                    }
                    None => {
                        // Don't count if peer was not previously known
                        false
                    }
                };

                self.peers.insert(peer::ReadInfo::get_id(peer), Arc::new(*peer));

                if increase_downloads {
                    self.downloaded += 1;
                    downloaded_stats_updated = true;
                }
            }
            _ => {
                drop(self.peers.insert(peer::ReadInfo::get_id(peer), Arc::new(*peer)));
            }
        }

        downloaded_stats_updated
    }

    fn remove_inactive_peers(&mut self, current_cutoff: DurationSinceUnixEpoch) {
        for entry in &self.peers {
            if entry.value().get_updated() >= current_cutoff {
                entry.remove();
            }
        }
    }
}
