//! libp2p Swarm setup: TCP, Identify, Kademlia, mDNS for node discovery and connectivity.

use crate::error::NetworkError;
use libp2p::identify::{Behaviour as IdentifyBehaviour, Config as IdentifyConfig};
use libp2p::kad::{store::MemoryStore, Behaviour as Kademlia};
use libp2p::mdns::tokio::Behaviour as Mdns;
use libp2p::swarm::NetworkBehaviour;
use libp2p::PeerId;
use std::time::Duration;

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
pub struct QuynBehaviour {
    pub identify: IdentifyBehaviour,
    pub kademlia: Kademlia<MemoryStore>,
    pub mdns: Mdns,
}

impl QuynBehaviour {
    /// Create behaviour with local peer id and public key for identify (sync; blocks on mdns).
    pub fn new_sync(
        local_peer_id: PeerId,
        public_key: libp2p::identity::PublicKey,
    ) -> Result<Self, NetworkError> {
        let identify_config = IdentifyConfig::new("/quyn/1.0".to_string(), public_key)
            .with_interval(Duration::from_secs(30))
            .with_push_listen_addr_updates(true);
        let identify = IdentifyBehaviour::new(identify_config);
        let store = MemoryStore::new(local_peer_id);
        let kademlia = Kademlia::new(local_peer_id, store);
        let mdns = Mdns::new(Default::default(), local_peer_id)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        Ok(Self {
            identify,
            kademlia,
            mdns,
        })
    }


    pub fn add_known_peer(&mut self, peer_id: PeerId, addr: libp2p::Multiaddr) {
        self.kademlia.add_address(&peer_id, addr);
    }
}

/// Build swarm using libp2p 0.53 SwarmBuilder (TCP + Noise + Yamux, QuynBehaviour).
pub fn build_swarm(
    keypair: libp2p::identity::Keypair,
) -> Result<libp2p::Swarm<QuynBehaviour>, NetworkError> {
    let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            Default::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )
        .map_err(|e| NetworkError::Protocol(e.to_string()))?
        .with_behaviour(|keypair| {
            QuynBehaviour::new_sync(keypair.public().to_peer_id(), keypair.public())
                .expect("quyn behaviour")
        })
        .map_err(|e| NetworkError::Protocol(e.to_string()))?
        .build();
    Ok(swarm)
}
