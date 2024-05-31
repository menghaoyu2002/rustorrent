use std::{collections::HashMap, fmt::Display, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinSet,
    time::timeout,
};

mod bitfield;
mod message;

use crate::{
    client::message::{receive_message, send_message},
    tracker::{Peer, Tracker},
};

use self::{
    bitfield::Bitfield,
    message::{Message, MessageId, SendMessageError},
};

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
    SendMessageError((Vec<u8>, SendMessageError)),
    ReceiveMessageError((Vec<u8>, Option<Message>, String)),
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
            ClientError::SendMessageError(e) => {
                write!(f, "SendMessageError: PeerId: {:?}, Error: {}", e.0, e.1)
            }
            ClientError::ReceiveMessageError(e) => {
                write!(
                    f,
                    "ReceiveMessageError: PeerId: {:?}, ReceivedMessage: {}, Reason: {}",
                    e.0,
                    e.1.clone().map_or("None".to_string(), |m| format!("{}", m)),
                    e.2
                )
            }
        }
    }
}

struct PeerState {
    peer_id: Vec<u8>,
    connection: TcpStream,
    bitfield: Bitfield,

    am_choking: bool,
    am_interested: bool,
    peer_choking: bool,
    peer_interested: bool,
}

pub struct Client {
    tracker: Tracker,
    peers: HashMap<Vec<u8>, PeerState>,
    bitfield: Bitfield,
}

impl Client {
    pub fn new(tracker: Tracker) -> Self {
        let bitfield = Bitfield::new(tracker.get_metainfo().get_peices().len());
        Self {
            tracker,
            peers: HashMap::new(),
            bitfield,
        }
    }

    pub async fn download(&mut self) -> Result<(), ClientError> {
        self.connect_to_peers(30).await?;
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

    async fn initiate_handshake(
        stream: &mut TcpStream,
        handshake: &Vec<u8>,
        info_hash: &Vec<u8>,
        peer: &Peer,
    ) -> Result<Vec<u8>, ClientError> {
        stream.write_all(handshake).await.map_err(|e| {
            ClientError::HandshakeError(HandshakeError {
                peer: peer.clone(),
                handshake: handshake.to_vec(),
                status: HandshakePhase::Send,
                message: format!("Failed to send handshake: {}", e),
            })
        })?;

        let mut response = vec![0u8; HANDSHAKE_LEN];
        stream.read_exact(&mut response).await.map_err(|e| {
            ClientError::HandshakeError(HandshakeError {
                peer: peer.clone(),
                handshake: handshake.to_vec(),
                status: HandshakePhase::Receive,
                message: format!("Failed to receive handshake: {}", e),
            })
        })?;

        Self::validate_handshake(&response, info_hash)
    }

    async fn connect_to_peers(&mut self, min_connections: usize) -> Result<(), ClientError> {
        println!("Connecting to peers...");

        while self.peers.len() < min_connections {
            let mut handles = JoinSet::new();
            for peer in self
                .tracker
                .get_peers()
                .await
                .map_err(|_| ClientError::GetPeersError(String::from("Failed to get peers")))?
            {
                let handshake = self.get_handshake()?;
                let info_hash = self.tracker.get_metainfo().get_info_hash().map_err(|_| {
                    ClientError::GetPeersError(String::from("Failed to get info hash"))
                })?;
                let bitfield = self.bitfield.to_bytes();
                let bitfield_len = self.bitfield.len();

                handles.spawn(async move {
                    let mut stream = match timeout(
                        Duration::from_secs(5),
                        TcpStream::connect(peer.addr),
                    )
                    .await
                    {
                        Ok(Ok(stream)) => stream,
                        Ok(Err(e)) => {
                            return Err(ClientError::GetPeersError(format!(
                                "Failed to connect to peer: {}",
                                e
                            )))
                        }
                        Err(_) => {
                            return Err(ClientError::GetPeersError(format!(
                                "Failed to connect to peer: {} - timed out",
                                peer.addr
                            )))
                        }
                    };

                    let peer_id =
                        Self::initiate_handshake(&mut stream, &handshake, &info_hash, &peer)
                            .await?;

                    send_message(&mut stream, Message::new(MessageId::Bitfield, &bitfield))
                        .await
                        .map_err(|e| ClientError::SendMessageError((peer_id.clone(), e)))?;
                    let message = receive_message(&mut stream)
                        .await
                        .map_err(|e| ClientError::SendMessageError((peer_id.clone(), e)))?;

                    let bitfield = match message.get_id() {
                        Ok(MessageId::Bitfield) => {
                            assert_eq!(message.get_payload().len(), bitfield_len.div_ceil(8));
                            Bitfield::from_bytes(message.get_payload(), bitfield_len)
                        }
                        _ => {
                            return Err(ClientError::ReceiveMessageError((
                                peer_id.clone(),
                                Some(message),
                                "Expected bitfield message".to_string(),
                            )))
                        }
                    };

                    Ok((peer_id, stream, bitfield))
                });
            }

            while let Some(handle) = handles.join_next().await {
                let conection_result =
                    handle.map_err(|e| ClientError::GetPeersError(format!("{}", e)))?;

                match conection_result {
                    Ok((peer_id, stream, bitfield)) => {
                        println!(
                            "Connected to peer: {:?}",
                            stream.peer_addr().map_err(|e| {
                                ClientError::GetPeersError(format!(
                                    "Failed to get peer address: {}",
                                    e
                                ))
                            })?
                        );
                        self.peers.insert(
                            peer_id.clone(),
                            PeerState {
                                peer_id,
                                connection: stream,
                                bitfield,
                                am_choking: true,
                                am_interested: false,
                                peer_choking: true,
                                peer_interested: false,
                            },
                        );
                    }
                    Err(e) => {
                        #[cfg(debug_assertions)]
                        eprintln!("{}", e);
                    }
                }
            }
        }

        println!("Connected to {} peers", self.peers.len());
        Ok(())
    }
}
