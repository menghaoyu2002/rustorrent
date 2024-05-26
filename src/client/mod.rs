use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

use crate::tracker::{Peer, Tracker};

const PSTR: &[u8; 19] = b"BitTorrent protocol";
const HANDSHAKE_LEN: usize = 49 + PSTR.len();

pub struct PeerConnectionError {
    pub peer: Peer,
}

pub enum HandshakePhase {
    Send,
    Receive,
}

impl Display for HandshakePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakePhase::Send => write!(f, "Send"),
            HandshakePhase::Receive => write!(f, "Receive"),
        }
    }
}

pub struct HandshakeError {
    peer: Peer,
    handshake: Vec<u8>,
    status: HandshakePhase,
    message: String,
}

pub enum ClientError {
    ValidateHandshakeError(String),
    GetPeersError(String),
    HandshakeError(HandshakeError),
}

impl Debug for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::ValidateHandshakeError(e) => write!(f, "ValidateHandshakeError: {}", e),
            ClientError::GetPeersError(e) => write!(f, "GetPeersError: {}", e),
            ClientError::HandshakeError(e) => write!(
                f,
                "HandshakeError: Peer: {}, Status: {}, Message: {} Handshake:\n{}",
                e.peer,
                e.status,
                e.message,
                e.handshake
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>()
            ),
        }
    }
}

pub struct Client {
    tracker: Tracker,
    connections: HashMap<Vec<u8>, TcpStream>,
}

impl Client {
    pub fn new(tracker: Tracker) -> Self {
        Self {
            tracker,
            connections: HashMap::new(),
        }
    }

    fn get_handshake(&self) -> Result<Vec<u8>, ClientError> {
        let mut handshake = Vec::new();

        let info_hash = self
            .tracker
            .get_metainfo()
            .get_info_hash()
            .map_err(|_| ClientError::GetPeersError(String::from("Failed to get info hash")))?;

        let peer_id = self.tracker.peer_id();

        handshake.push(PSTR.len() as u8);
        handshake.extend_from_slice(PSTR);
        handshake.extend_from_slice(&[0; 8]);
        handshake.extend_from_slice(&info_hash);
        handshake.extend_from_slice(peer_id.as_bytes());

        assert_eq!(handshake.len(), HANDSHAKE_LEN);
        Ok(handshake)
    }

    fn validate_handshake(handshake: &[u8], info_hash: &Vec<u8>) -> Result<Vec<u8>, ClientError> {
        if handshake.len() != HANDSHAKE_LEN {
            return Err(ClientError::ValidateHandshakeError(
                "Invalid handshake length".to_string(),
            ));
        }

        let pstr_len = handshake[0] as usize;
        if pstr_len != b"BitTorrent protocol".len() {
            return Err(ClientError::ValidateHandshakeError(
                "Invalid protocol string length".to_string(),
            ));
        }

        if &handshake[1..20] != b"BitTorrent protocol" {
            return Err(ClientError::ValidateHandshakeError(
                "Invalid protocol string".to_string(),
            ));
        }

        if &handshake[28..48] != info_hash {
            return Err(ClientError::ValidateHandshakeError(
                "Invalid info hash".to_string(),
            ));
        }

        let peer_id = handshake[48..68].to_vec();

        Ok(peer_id)
    }

    pub async fn connect_to_peers(&mut self, min_connections: usize) -> Result<(), ClientError> {
        while self.connections.len() < min_connections {
            let mut handles = Vec::new();
            for peer in self
                .tracker
                .get_peers()
                .await
                .map_err(|_| ClientError::GetPeersError(String::from("Failed to get peers")))?
            {
                if self.connections.len() >= min_connections {
                    break;
                }

                let handshake = self.get_handshake()?;
                let info_hash = self.tracker.get_metainfo().get_info_hash().map_err(|_| {
                    ClientError::GetPeersError(String::from("Failed to get info hash"))
                })?;

                let handle = tokio::spawn(async move {
                    match TcpStream::connect_timeout(&peer.addr, Duration::new(5, 0)) {
                        Ok(mut stream) => {
                            stream.write_all(&handshake).map_err(|e| {
                                ClientError::HandshakeError(HandshakeError {
                                    peer: peer.clone(),
                                    handshake: handshake.clone(),
                                    status: HandshakePhase::Send,
                                    message: e.to_string(),
                                })
                            })?;

                            let mut handshake_response = [0u8; HANDSHAKE_LEN];
                            stream.read_exact(&mut handshake_response).map_err(|e| {
                                ClientError::HandshakeError(HandshakeError {
                                    peer: peer.clone(),
                                    handshake: handshake_response.to_vec(),
                                    status: HandshakePhase::Receive,
                                    message: e.to_string(),
                                })
                            })?;

                            let peer_id =
                                Client::validate_handshake(&handshake_response, &info_hash)?;

                            Ok((peer_id, stream))
                        }
                        Err(_) => Err(ClientError::GetPeersError(format!(
                            "Failed to connect to peer {}",
                            peer.addr
                        ))),
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                if self.connections.len() >= min_connections {
                    break;
                }

                match handle
                    .await
                    .map_err(|e| ClientError::GetPeersError(String::from(e.to_string())))?
                {
                    Ok((peer_id, stream)) => {
                        println!(
                            "Connected to peer: {} at {}",
                            String::from_utf8_lossy(&peer_id),
                            stream
                                .peer_addr()
                                .map(|addr| addr.to_string())
                                .unwrap_or("Unknown".to_string())
                        );
                        self.connections.insert(peer_id, stream);
                    }
                    Err(e) => {
                        eprintln!("{:?}", e);
                    }
                }
            }
        }

        Ok(())
    }
}
