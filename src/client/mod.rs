use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Utc};
use futures::future::join_all;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{Mutex, RwLock},
    task::{yield_now, JoinHandle, JoinSet},
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
    ProcessMessagesError(String),
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
            ClientError::ProcessMessagesError(e) => write!(f, "ProcessMessagesError: {}", e),
        }
    }
}

struct PeerState {
    peer_id: Vec<u8>,
    stream: Arc<Mutex<TcpStream>>,
    bitfield: Option<Bitfield>,
    send_queue: Arc<Mutex<VecDeque<Message>>>,
    receive_queue: Arc<Mutex<VecDeque<Message>>>,
    last_sent: DateTime<Utc>,

    am_choking: bool,
    am_interested: bool,
    peer_choking: bool,
    peer_interested: bool,
}

impl PeerState {
    pub fn new(peer_id: &Vec<u8>, stream: TcpStream) -> Self {
        Self {
            peer_id: peer_id.clone(),
            stream: Arc::new(Mutex::new(stream)),
            send_queue: Arc::new(Mutex::new(VecDeque::new())),
            receive_queue: Arc::new(Mutex::new(VecDeque::new())),
            last_sent: Utc::now(),

            bitfield: None,
            am_choking: true,
            am_interested: false,
            peer_choking: true,
            peer_interested: false,
        }
    }
}

pub struct Client {
    tracker: Tracker,
    peers: Arc<RwLock<HashMap<Vec<u8>, PeerState>>>,
    bitfield: Bitfield,
}

impl Client {
    pub fn new(tracker: Tracker) -> Self {
        let bitfield = Bitfield::new(tracker.get_metainfo().get_peices().len());
        Self {
            tracker,
            peers: Arc::new(RwLock::new(HashMap::new())),
            bitfield,
        }
    }

    pub async fn download(&mut self) -> Result<(), ClientError> {
        let peer_ids = self.connect_to_peers(30).await?;

        let mut handles = Vec::new();
        for id in peer_ids {
            handles.push(self.send_messages(&id).await);
            handles.push(self.retrieve_messages(&id));
        }

        join_all(handles).await;

        Ok(())
    }

    fn retrieve_messages(&self, peer_id: &Vec<u8>) -> JoinHandle<Vec<u8>> {
        let peers = Arc::clone(&self.peers);
        let id = peer_id.clone();

        tokio::spawn(async move {
            loop {
                let id_to_peer = peers.read().await;
                let Some(peer) = id_to_peer.get(&id) else {
                    break;
                };
                let mut stream = peer.stream.lock().await;
                let Ok(message) = receive_message(&mut stream).await else {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "Failed to receive message from peer: {}",
                        String::from_utf8_lossy(&id)
                    );
                    break;
                };

                println!(
                    "Received \"{}\" message from peer: {}",
                    &message.get_id(),
                    String::from_utf8_lossy(&id)
                );

                peer.receive_queue.lock().await.push_back(message);
            }
            // peers.write().await.remove(&id);
            id
        })
    }

    async fn send_messages(&self, peer_id: &Vec<u8>) -> JoinHandle<Vec<u8>> {
        let peers = Arc::clone(&self.peers);
        let id = peer_id.clone();
        tokio::spawn(async move {
            loop {
                let id_to_peer = peers.read().await;
                let Some(peer) = id_to_peer.get(&id) else {
                    break;
                };
                let message = match peer.send_queue.lock().await.pop_front() {
                    Some(m) => m,
                    None => {
                        if (peer.last_sent - Utc::now()).num_seconds() > 120 {
                            Message::new(MessageId::KeepAlive, &Vec::new())
                        } else {
                            yield_now().await;
                            continue;
                        }
                    }
                };

                println!(
                    "Sending message {} to peer: {}",
                    message.get_id(),
                    String::from_utf8_lossy(&id)
                );

                let mut stream = peer.stream.lock().await;
                if let Err(e) = send_message(&mut stream, message).await {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "Failed to send message to peer {:?}: {}",
                        String::from_utf8_lossy(&id),
                        e.to_string()
                    );
                    break;
                }
            }
            // peers.write().await.remove(&id);
            id
        })
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

    async fn connect_to_peers(
        &mut self,
        min_connections: usize,
    ) -> Result<Vec<Vec<u8>>, ClientError> {
        println!("Connecting to peers...");
        let mut new_peers = Vec::new();
        while self.peers.read().await.len() < min_connections {
            let mut handles = JoinSet::new();
            for peer in
                self.tracker.get_peers().await.map_err(|e| {
                    ClientError::GetPeersError(format!("Failed to get peers: {}", e))
                })?
            {
                if self.peers.read().await.len() >= min_connections {
                    break;
                }
                let handshake = self.get_handshake()?;
                let info_hash = self.tracker.get_metainfo().get_info_hash().map_err(|_| {
                    ClientError::GetPeersError(String::from("Failed to get info hash"))
                })?;
                let bitfield = self.bitfield.to_bytes();

                let peers = Arc::clone(&mut self.peers);

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

                    if peers.read().await.len() >= min_connections {
                        return Err(ClientError::GetPeersError(String::from(
                            "Already connected to minimum number of peers",
                        )));
                    }

                    let peer_state = PeerState::new(&peer_id, stream);
                    peer_state
                        .send_queue
                        .lock()
                        .await
                        .push_back(Message::new(MessageId::Bitfield, &bitfield));
                    peers.write().await.insert(peer_id.clone(), peer_state);

                    println!("Connected to peer: {:?}", peer.addr);

                    Ok(peer_id)
                });
            }

            while let Some(handle) = handles.join_next().await {
                let conection_result =
                    handle.map_err(|e| ClientError::GetPeersError(format!("{}", e)))?;

                match conection_result {
                    Ok(peer_id) => {
                        new_peers.push(peer_id);
                    }
                    Err(e) => {
                        // #[cfg(debug_assertions)]
                        // eprintln!("{}", e);
                    }
                }
            }
        }

        println!("Connected to {} new peers", new_peers.len());
        Ok(new_peers)
    }
}
