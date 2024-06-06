use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Utc};
use pieces::PieceScheduler;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{Mutex, RwLock},
    task::{yield_now, JoinHandle, JoinSet},
    time::timeout,
};

mod bitfield;
mod message;
mod pieces;

use crate::{
    client::message::{receive_message, send_message},
    tracker::{Peer, Tracker},
};

use self::{
    bitfield::Bitfield,
    message::{Message, MessageId, ReceiveError, SendError, SendMessageError},
};

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
    stream: TcpStream,
    bitfield: Option<Bitfield>,
    last_touch: DateTime<Utc>,

    am_choking: bool,
    am_interested: bool,
    peer_choking: bool,
    peer_interested: bool,
}

impl PeerState {
    pub fn new(peer_id: &Vec<u8>, stream: TcpStream) -> Self {
        Self {
            peer_id: peer_id.clone(),
            stream,
            last_touch: Utc::now(),

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
    peers: Arc<RwLock<HashMap<Vec<u8>, Arc<RwLock<PeerState>>>>>,
    piece_scheduler: Arc<RwLock<PieceScheduler>>,
    send_queue: Arc<Mutex<VecDeque<(Vec<u8>, Message)>>>,
    receive_queue: Arc<Mutex<VecDeque<(Vec<u8>, Message)>>>,
}

impl Client {
    pub fn new(tracker: Tracker) -> Self {
        let piece_scheduler = PieceScheduler::new(&tracker.get_metainfo().info);
        Self {
            tracker,
            peers: Arc::new(RwLock::new(HashMap::new())),
            piece_scheduler: Arc::new(RwLock::new(piece_scheduler)),
            send_queue: Arc::new(Mutex::new(VecDeque::new())),
            receive_queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub async fn download(&mut self) -> Result<(), ClientError> {
        self.connect_to_peers(30).await?;

        let _ = tokio::join!(
            self.send_messages(),
            self.retrieve_messages(),
            self.process_messages(),
            self.keep_alive(),
        );

        Ok(())
    }

    async fn process_messages(&self) -> JoinHandle<()> {
        let peers = Arc::clone(&self.peers);
        let receive_queue = Arc::clone(&self.receive_queue);
        let piece_scheduler = Arc::clone(&self.piece_scheduler);
        let num_pieces = self.piece_scheduler.read().await.len();
        let send_queue = Arc::clone(&self.send_queue);

        tokio::spawn(async move {
            loop {
                let Some((peer_id, message)) = receive_queue.lock().await.pop_front() else {
                    yield_now().await;
                    continue;
                };

                let mut should_remove = false;

                {
                    let id_to_peer = peers.read().await;
                    let Some(peer) = id_to_peer.get(&peer_id) else {
                        continue;
                    };

                    let message_id = message.get_id();
                    println!(
                        "Processing \"{}\" message from {}",
                        message_id,
                        String::from_utf8_lossy(&peer_id)
                    );
                    match message_id {
                        MessageId::Choke => {
                            peer.write().await.peer_choking = true;
                        }
                        MessageId::Unchoke => {
                            peer.write().await.peer_choking = false;

                            let scheduled_piece =
                                piece_scheduler.write().await.schedule_piece(&peer_id);

                            match scheduled_piece {
                                Some((index, begin, length)) => {
                                    if !peer.read().await.peer_choking {
                                        let mut payload = Vec::new();
                                        payload.extend_from_slice(&index.to_be_bytes());
                                        payload.extend_from_slice(&begin.to_be_bytes());
                                        payload.extend_from_slice(&length.to_be_bytes());
                                        let message = Message::new(MessageId::Request, &payload);
                                        send_queue
                                            .lock()
                                            .await
                                            .push_back((peer_id.clone(), message));
                                    }
                                }
                                None => send_queue.lock().await.push_back((
                                    peer_id.clone(),
                                    Message::new(MessageId::NotInterested, &Vec::new()),
                                )),
                            };
                        }
                        MessageId::Interested => {
                            peer.write().await.peer_interested = true;
                            // figure out how to choke
                        }
                        MessageId::NotInterested => {
                            let mut peer = peer.write().await;
                            peer.peer_interested = false;
                            peer.am_choking = true;
                        }
                        MessageId::Have => {
                            let payload = message.get_payload();
                            let piece_index = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                            if peer.write().await.bitfield.is_none() {
                                peer.write().await.bitfield = Some(Bitfield::new(num_pieces));
                            };

                            if let Some(bitfield) = &mut peer.write().await.bitfield {
                                should_remove = bitfield.set(piece_index as usize, true).is_err();
                                if piece_scheduler.read().await.is_interested(bitfield) {
                                    send_queue.lock().await.push_back((
                                        peer_id.clone(),
                                        Message::new(MessageId::Interested, &Vec::new()),
                                    ));
                                } else {
                                    send_queue.lock().await.push_back((
                                        peer_id.clone(),
                                        Message::new(MessageId::NotInterested, &Vec::new()),
                                    ));
                                }
                            }

                            piece_scheduler
                                .write()
                                .await
                                .add_peer_have(&peer_id, piece_index as usize);
                        }
                        MessageId::Bitfield => {
                            let payload = message.get_payload();
                            if payload.len() * 8 < num_pieces {
                                println!("Invalid bitfield length, disconnecting peer...");
                                should_remove = true;
                            } else {
                                let bitfield = Bitfield::from_bytes(payload, num_pieces);

                                piece_scheduler
                                    .write()
                                    .await
                                    .add_peer_count(&peer_id, &bitfield);

                                if piece_scheduler.read().await.is_interested(&bitfield) {
                                    send_queue.lock().await.push_back((
                                        peer_id.clone(),
                                        Message::new(MessageId::Interested, &Vec::new()),
                                    ));
                                } else {
                                    send_queue.lock().await.push_back((
                                        peer_id.clone(),
                                        Message::new(MessageId::NotInterested, &Vec::new()),
                                    ));
                                }

                                peer.write().await.bitfield = Some(bitfield);
                            }
                        }
                        MessageId::Request => {}
                        MessageId::Piece => {
                            let payload = message.get_payload();
                            let index = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                            let begin = u32::from_be_bytes(payload[4..8].try_into().unwrap());
                            let block = &payload[8..];
                            piece_scheduler.write().await.set_block(
                                index as usize,
                                begin,
                                block.to_vec(),
                            );

                            let peer = peer.read().await;
                            if piece_scheduler
                                .read()
                                .await
                                .is_interested(peer.bitfield.as_ref().unwrap())
                            {
                                if peer.peer_choking {
                                    send_queue.lock().await.push_back((
                                        peer_id.clone(),
                                        Message::new(MessageId::Interested, &Vec::new()),
                                    ));
                                } else {
                                    if let Some((index, begin, length)) =
                                        piece_scheduler.write().await.schedule_piece(&peer_id)
                                    {
                                        let mut payload = Vec::new();
                                        payload.extend_from_slice(&index.to_be_bytes());
                                        payload.extend_from_slice(&begin.to_be_bytes());
                                        payload.extend_from_slice(&length.to_be_bytes());
                                        send_queue.lock().await.push_back((
                                            peer_id.clone(),
                                            Message::new(MessageId::Request, &payload),
                                        ));
                                    }
                                }
                            } else {
                                if !peer.peer_choking {
                                    send_queue.lock().await.push_back((
                                        peer_id.clone(),
                                        Message::new(MessageId::NotInterested, &Vec::new()),
                                    ));
                                }
                            }
                        }
                        MessageId::Cancel => {}
                        MessageId::KeepAlive => {}
                        MessageId::Port => {}
                    }
                }

                if should_remove {
                    peers.write().await.remove(&peer_id);
                    piece_scheduler.write().await.remove_peer_count(&peer_id);
                }

                yield_now().await;
            }
        })
    }

    fn keep_alive(&self) -> JoinHandle<()> {
        let peers = Arc::clone(&self.peers);
        let send_queue = Arc::clone(&self.send_queue);
        tokio::spawn(async move {
            loop {
                for (peer_id, peer) in peers.read().await.iter() {
                    if (Utc::now() - peer.read().await.last_touch).num_seconds() > 60 {
                        send_queue.lock().await.push_back((
                            peer_id.clone(),
                            Message::new(MessageId::KeepAlive, &Vec::new()),
                        ));
                    }
                }
            }
        })
    }

    fn retrieve_messages(&self) -> JoinHandle<()> {
        let peers = Arc::clone(&self.peers);
        let receive_queue = Arc::clone(&self.receive_queue);
        let piece_scheduler = Arc::clone(&self.piece_scheduler);
        tokio::spawn(async move {
            let mut peers_to_remove = Vec::new();
            loop {
                for (peer_id, peer) in peers.read().await.iter() {
                    {
                        let stream = &peer.read().await.stream;
                        match receive_message(stream).await {
                            Ok(message) => {
                                println!(
                                    "Received \"{}\" message from {}",
                                    message.get_id(),
                                    String::from_utf8_lossy(peer_id)
                                );
                                receive_queue
                                    .lock()
                                    .await
                                    .push_back((peer_id.clone(), message));
                            }
                            Err(ReceiveError::WouldBlock) => {
                                yield_now().await;
                                continue;
                            }
                            Err(e) => {
                                println!(
                                    "Failed to receive message from peer {:?}: {}",
                                    String::from_utf8_lossy(peer_id),
                                    e.to_string()
                                );
                                peers_to_remove.push(peer_id.clone());
                            }
                        }
                    }

                    peer.write().await.last_touch = Utc::now();
                }
                yield_now().await;

                for peer_id in &peers_to_remove {
                    if peers.write().await.remove(peer_id).is_some() {
                        piece_scheduler.write().await.remove_peer_count(&peer_id);
                        println!(
                            "Disconnected from peer: {:?}",
                            String::from_utf8_lossy(&peer_id)
                        );
                    }
                }
            }
        })
    }

    fn send_messages(&self) -> JoinHandle<()> {
        let peers = Arc::clone(&self.peers);
        let send_queue = Arc::clone(&self.send_queue);
        let piece_scheduler = Arc::clone(&self.piece_scheduler);
        tokio::spawn(async move {
            loop {
                let Some((peer_id, message)) = send_queue.lock().await.pop_front() else {
                    yield_now().await;
                    continue;
                };

                let send_result = {
                    let id_to_peer = peers.read().await;
                    let Some(peer) = id_to_peer.get(&peer_id) else {
                        // if peer is not found, discard the message
                        continue;
                    };

                    let stream = &peer.read().await.stream;
                    println!(
                        "Sending \"{}\" message from {}",
                        message.get_id(),
                        String::from_utf8_lossy(&peer_id)
                    );
                    send_message(stream, &message).await
                };

                match send_result {
                    Ok(()) => {
                        let id_to_peer = peers.read().await;
                        let mut peer = id_to_peer.get(&peer_id).unwrap().write().await;
                        peer.last_touch = Utc::now();
                    }
                    Err(SendError::WouldBlock) => {
                        send_queue.lock().await.push_back((peer_id, message));
                        yield_now().await;
                    }
                    Err(_) => {
                        println!(
                            "Failed to send message to peer: {:?}",
                            String::from_utf8_lossy(&peer_id)
                        );
                        if peers.write().await.remove(&peer_id).is_some() {
                            piece_scheduler.write().await.remove_peer_count(&peer_id);
                            println!(
                                "Disconnected from peer: {:?}",
                                String::from_utf8_lossy(&peer_id)
                            );
                        }
                    }
                }
            }
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

    async fn connect_to_peers(&mut self, min_connections: usize) -> Result<(), ClientError> {
        println!("Connecting to peers...");
        while self.peers.read().await.len() < min_connections {
            let mut handles = JoinSet::new();
            for peer in
                self.tracker.get_peers().await.map_err(|e| {
                    ClientError::GetPeersError(format!("Failed to get peers: {}", e))
                })?
            {
                let handshake = self.get_handshake()?;
                let info_hash = self.tracker.get_metainfo().get_info_hash().map_err(|_| {
                    ClientError::GetPeersError(String::from("Failed to get info hash"))
                })?;
                let bitfield = self.piece_scheduler.read().await.to_bitfield().to_bytes();

                let peers = Arc::clone(&mut self.peers);
                let send_queue = Arc::clone(&self.send_queue);

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

                    send_queue.lock().await.push_back((
                        peer_id.clone(),
                        Message::new(MessageId::Bitfield, &bitfield),
                    ));
                    peers.write().await.insert(
                        peer_id.clone(),
                        Arc::new(RwLock::new(PeerState::new(&peer_id, stream))),
                    );

                    println!("Connected to peer: {:?}", peer.addr);

                    Ok(peer_id)
                });
            }

            while let Some(handle) = handles.join_next().await {
                let conection_result =
                    handle.map_err(|e| ClientError::GetPeersError(format!("{}", e)))?;

                if let Err(e) = conection_result {
                    // #[cfg(debug_assertions)]
                    // eprintln!("{}", e);
                }
            }
        }

        println!("Connected to {} new peers", self.peers.read().await.len());
        Ok(())
    }
}
