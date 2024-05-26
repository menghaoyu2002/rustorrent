use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

use crate::tracker::Tracker;

const HANDSHAKE_LEN: usize = 68;

pub struct Client {
    tracker: Tracker,
    connections: Vec<TcpStream>,
}

impl Client {
    pub fn new(tracker: Tracker) -> Result<Self, String> {
        Ok(Self {
            tracker,
            connections: Vec::new(),
        })
    }

    fn get_handshake(&self) -> Result<Vec<u8>, String> {
        let mut handshake = Vec::new();

        let pstr = b"BitTorrent protocol";
        let info_hash = self.tracker.get_metainfo().get_info_hash()?;
        let peer_id = self.tracker.peer_id();

        handshake.push(pstr.len() as u8);
        handshake.extend_from_slice(pstr);
        handshake.extend_from_slice(&[0; 8]);
        handshake.extend_from_slice(&info_hash);
        handshake.extend_from_slice(peer_id.as_bytes());

        assert_eq!(handshake.len(), HANDSHAKE_LEN);
        Ok(handshake)
    }

    pub async fn connect_to_peers(&mut self, min_connections: usize) -> Result<(), String> {
        while self.connections.len() < min_connections {
            let mut handles = Vec::new();
            for peer in self.tracker.get_peers().await? {
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

                            let mut buf = [0u8; HANDSHAKE_LEN];
                            match stream.read_exact(&mut buf) {
                                Ok(()) => {
                                    println!(
                                        "Received handshake from peer {} {:#?}",
                                        peer.addr,
                                        String::from_utf8_lossy(&buf)
                                    );
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

                match handle.await.map_err(|e| e.to_string())? {
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
