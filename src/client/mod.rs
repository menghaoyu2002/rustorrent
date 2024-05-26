use std::{
    fmt::Debug,
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

use crate::tracker::{Peer, Tracker};

const HANDSHAKE_LEN: usize = 68;

pub struct PeerConnectionError {
    pub peer: Peer,
}

pub enum ClientError {
    ValidateHandshakeError(String),
    GetPeersError(String),
    HandshakeError,
}

impl Debug for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::ValidateHandshakeError(e) => write!(f, "ValidateHandshakeError: {}", e),
            ClientError::GetPeersError(e) => write!(f, "GetPeersError: {}", e),
            ClientError::HandshakeError => write!(f, "HandshakeError"),
        }
    }
}

pub struct Client {
    tracker: Tracker,
    connections: Vec<TcpStream>,
}

impl Client {
    pub fn new(tracker: Tracker) -> Self {
        Self {
            tracker,
            connections: Vec::new(),
        }
    }

    fn get_handshake(&self) -> Result<Vec<u8>, ClientError> {
        let mut handshake = Vec::new();

        let pstr = b"BitTorrent protocol";
        let info_hash = self
            .tracker
            .get_metainfo()
            .get_info_hash()
            .map_err(|_| ClientError::HandshakeError)?;
        let peer_id = self.tracker.peer_id();

        handshake.push(pstr.len() as u8);
        handshake.extend_from_slice(pstr);
        handshake.extend_from_slice(&[0; 8]);
        handshake.extend_from_slice(&info_hash);
        handshake.extend_from_slice(peer_id.as_bytes());

        assert_eq!(handshake.len(), HANDSHAKE_LEN);
        Ok(handshake)
    }

    fn validate_handshake(&self, handshake: &[u8]) -> Result<String, ClientError> {
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

        if &handshake[20..28] != [0u8; 8] {
            return Err(ClientError::ValidateHandshakeError(
                "Invalid reserved bytes".to_string(),
            ));
        }

        let info_hash = self
            .tracker
            .get_metainfo()
            .get_info_hash()
            .map_err(|_| ClientError::HandshakeError)?;

        if &handshake[28..48] != info_hash {
            return Err(ClientError::ValidateHandshakeError(
                "Invalid info hash".to_string(),
            ));
        }

        let peer_id = String::from_utf8(handshake[48..68].to_vec())
            .map_err(|_| ClientError::HandshakeError)?;

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
                let handle = tokio::spawn(async move {
                    match TcpStream::connect_timeout(&peer.addr, Duration::new(5, 0)) {
                        Ok(mut stream) => {
                            println!("Connected to peer {}", peer.addr);
                            match stream.write_all(&handshake) {
                                Ok(_) => {
                                    println!("Handshake sent to peer {}", peer.addr,);
                                }
                                Err(_) => {
                                    return Err(format!(
                                        "Failed to send handshake to peer {}",
                                        peer.addr
                                    ))
                                }
                            }

                            // if self.validate_handshake(&handshake).is_err() {
                            //     return Err(format!("Invalid handshake from peer {}", peer.addr));
                            // }

                            let mut buf = [0u8; HANDSHAKE_LEN];
                            match stream.read_exact(&mut buf) {
                                Ok(()) => {
                                    println!("Received handshake from peer {}", peer.addr,);
                                }
                                Err(_) => {
                                    return Err(format!(
                                        "Failed to read handshake from peer {}",
                                        peer.addr
                                    ))
                                }
                            }

                            Ok(stream)
                        }
                        Err(_) => Err(format!("Failed to connect to peer {}", peer.addr)),
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
                    .map_err(|e| ClientError::GetPeersError(e.to_string()))?
                {
                    Ok(stream) => {
                        self.connections.push(stream);
                    }
                    Err(e) => {
                        eprintln!("{}", e)
                    }
                }
            }
        }

        Ok(())
    }
}
