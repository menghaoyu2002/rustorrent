use std::{net::TcpStream, time::Duration};

use crate::tracker::Tracker;

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

    pub async fn connect_to_peers(&mut self, min_connections: usize) -> Result<(), String> {
        while self.connections.len() < min_connections {
            let mut handles = Vec::new();
            for peer in self.tracker.get_peers().await? {
                if self.connections.len() >= min_connections {
                    break;
                }

                let handle = tokio::spawn(async move {
                    match TcpStream::connect_timeout(&peer.addr, Duration::new(5, 0)) {
                        Ok(stream) => {
                            println!("Connected to peer {}", peer.addr);
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
