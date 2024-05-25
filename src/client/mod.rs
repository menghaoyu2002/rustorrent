use std::{net::TcpStream, time::Duration};

use crate::tracker::Tracker;

pub struct Client {
    tracker: Tracker,
    connections: Vec<TcpStream>,
}

impl Client {
    pub async fn new(tracker: Tracker) -> Result<Self, String> {
        Ok(Self {
            tracker,
            connections: Vec::new(),
        })
    }

    pub async fn connect_to_peers(&mut self, min_connections: usize) -> Result<(), String> {
        while self.connections.len() < min_connections {
            for peer in &self.tracker.get_peers().await? {
                if self.connections.len() >= min_connections {
                    break;
                }

                if let Ok(stream) = TcpStream::connect_timeout(&peer.addr, Duration::new(5, 0)) {
                    println!("Connected to peer {}", peer.addr);
                    self.connections.push(stream);
                } else {
                    println!("Failed to connect to peer {}", peer.addr);
                }
            }
        }

        Ok(())
    }
}
