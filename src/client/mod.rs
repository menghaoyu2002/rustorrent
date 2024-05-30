use std::{
    collections::HashMap,
    fmt::Display,
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

use crate::tracker::{Peer, Tracker};

const PSTR: &[u8; 19] = b"BitTorrent protocol";
const HANDSHAKE_LEN: usize = 49 + PSTR.len();
const REQUEST_LEN: u32 = 2 << 14;

pub struct PeerConnectionError {
    pub peer: Peer,
}

pub enum HandshakePhase {
    Send,
    Receive,
}

pub enum MessageId {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
    Port = 9,
}

impl MessageId {
    fn value(&self) -> u8 {
        match self {
            MessageId::Choke => 0,
            MessageId::Unchoke => 1,
            MessageId::Interested => 2,
            MessageId::NotInterested => 3,
            MessageId::Have => 4,
            MessageId::Bitfield => 5,
            MessageId::Request => 6,
            MessageId::Piece => 7,
            MessageId::Cancel => 8,
            MessageId::Port => 9,
        }
    }
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

pub struct SendMessageError {
    peer: Peer,
    message: Message,
    error: String,
}

pub enum ClientError {
    ValidateHandshakeError(String),
    GetPeersError(String),
    HandshakeError(HandshakeError),
    SendMessageError(SendMessageError),
}

impl Display for ClientError {
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
            ClientError::SendMessageError(e) => write!(
                f,
                "SendMessageError: Peer: {}, Message: {:02x?}, Error: {}",
                e.peer,
                e.message.serialize(),
                e.error
            ),
        }
    }
}

struct Message {
    len: u32,
    id: u8,
    payload: Vec<u8>,
}

impl Message {
    fn new(id: MessageId, payload: &Vec<u8>) -> Self {
        Self {
            len: payload.len() as u32 + 1,
            id: id.value(),
            payload: payload.clone(),
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(&self.len.to_be_bytes());
        message.push(self.id);
        message.extend_from_slice(&self.payload);
        message
    }
}

impl Clone for Message {
    fn clone(&self) -> Self {
        Self {
            len: self.len,
            id: self.id,
            payload: self.payload.clone(),
        }
    }
}

pub struct Client {
    tracker: Tracker,
    connections: HashMap<Vec<u8>, TcpStream>,
    bitfield: Vec<u8>,
}

impl Client {
    pub fn new(tracker: Tracker) -> Self {
        // divide the number of pieces by 8 to get the number of bytes needed to represent the bitfield
        let bitfield = vec![0u8; tracker.get_metainfo().get_peices().len().div_ceil(8)];
        Self {
            tracker,
            connections: HashMap::new(),
            bitfield,
        }
    }

    pub async fn download(&mut self) -> Result<(), ClientError> {
        println!("Starting download...");
        // self.connect_to_peers(30).await?;
        self.connect_to_peers(1).await?;
        self.send_message(Message::new(MessageId::Bitfield, &self.bitfield))?;
        self.send_message(Message::new(MessageId::Interested, &Vec::new()))?;
        Ok(())
    }

    fn send_message(&mut self, message: Message) -> Result<(), ClientError> {
        let serialized_message = message.serialize();
        println!("Sending message: {:?}", serialized_message);
        for (_, stream) in self.connections.iter_mut() {
            stream.write_all(&serialized_message).map_err(|e| {
                ClientError::SendMessageError(SendMessageError {
                    peer: Peer {
                        peer_id: None,
                        addr: stream.peer_addr().unwrap(),
                    },
                    message: message.clone(),
                    error: e.to_string(),
                })
            })?;

            let mut response = vec![0u8; serialized_message.len()];
            stream.read_exact(&mut response).map_err(|e| {
                ClientError::GetPeersError(format!("Failed to read response: {}", e))
            })?;
            println!("Response: {:?}", response);
        }
        Ok(())
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
        handshake.extend_from_slice(&peer_id);

        #[cfg(debug_assertions)]
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

    async fn connect_to_peers(&mut self, min_connections: usize) -> Result<(), ClientError> {
        println!("Connecting to peers...");
        while self.connections.len() < min_connections {
            let mut handles = Vec::new();
            for peer in self
                .tracker
                .get_peers()
                .await
                .map_err(|_| ClientError::GetPeersError(String::from("Failed to get peers")))?
            {
                if self.connections.len() >= min_connections {
                    return Ok(());
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
                    handle.abort();
                } else {
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
                            #[cfg(debug_assertions)]
                            eprintln!("{}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
