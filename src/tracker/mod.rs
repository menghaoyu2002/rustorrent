use rand::Rng;
use sha1::{Digest, Sha1};

use crate::bencode::{BencodeString, BencodeValue, Metainfo};

#[derive(Debug)]
pub struct Tracker {
    torrent_content: BencodeValue,
    metainfo: Metainfo,
    peer_id: String,
}

#[derive(Debug)]
pub struct PeerDict {
    ip: String,
    port: i64,
    peer_id: String,
}

#[derive(Debug)]
pub enum Peers {
    Dict(Vec<PeerDict>),
    Binary(Vec<String>),
}

#[derive(Debug)]
pub struct TrackerSuccessResponse {
    interval: i64,
    min_interval: Option<i64>,
    tracker_id: Option<String>,
    complete: i64,
    incomplete: i64,
    peers: Peers,
}

#[derive(Debug)]
pub struct TrackerFailureResponse {
    failure_reason: String,
}

#[derive(Debug)]
pub enum TrackerResponse {
    Success(TrackerSuccessResponse),
    Failure(TrackerFailureResponse),
}

impl Tracker {
    pub fn new(torrent_content: BencodeValue) -> Self {
        let metainfo = torrent_content.to_metainfo().expect("Invalid metainfo");

        Self {
            torrent_content,
            metainfo,
            peer_id: Tracker::get_peer_id(),
        }
    }

    fn parse_peers(value: &BencodeValue) -> Result<Peers, String> {
        match value {
            BencodeValue::String(BencodeString::Bytes(peers)) => {
                let mut ips = Vec::new();
                for peer in peers.chunks(6) {
                    let ip = format!("{}.{}.{}.{}", peer[0], peer[1], peer[2], peer[3]);
                    let port = u16::from(peer[4]) << 8 | u16::from(peer[5]);
                    ips.push(format!("{}:{}", ip, port));
                }
                return Ok(Peers::Binary(ips));
            }
            BencodeValue::List(peers) => {
                let mut parsed_peers = Vec::new();
                for peer in peers {
                    match peer {
                        BencodeValue::Dict(dict) => {
                            let ip = match dict.get("ip") {
                                Some(BencodeValue::String(BencodeString::String(ip))) => ip.clone(),
                                _ => return Err("ip key not found".to_string()),
                            };

                            let port = match dict.get("port") {
                                Some(BencodeValue::Int(port)) => *port,
                                _ => return Err("port key not found".to_string()),
                            };

                            let peer_id = match dict.get("peer id") {
                                Some(BencodeValue::String(BencodeString::String(peer_id))) => {
                                    peer_id.clone()
                                }
                                _ => return Err("peer id key not found".to_string()),
                            };

                            parsed_peers.push(PeerDict { ip, port, peer_id });
                        }
                        _ => return Err("invalid peers".to_string()),
                    }
                }
                return Ok(Peers::Dict(parsed_peers));
            }
            _ => return Err("invalid peers".to_string()),
        }
    }

    fn parse_success_response(value: &BencodeValue) -> Result<TrackerSuccessResponse, String> {
        let interval = match value.get_value("interval") {
            Some(interval) => match interval {
                BencodeValue::Int(interval) => *interval,
                _ => unreachable!(),
            },
            None => return Err("interval key not found".to_string()),
        };

        let min_interval = match value.get_value("min interval") {
            Some(min_interval) => match min_interval {
                BencodeValue::Int(min_interval) => Some(*min_interval),
                _ => return Err("min interval key not found".to_string()),
            },
            None => None,
        };

        let tracker_id = match value.get_value("tracker id") {
            Some(tracker_id) => match tracker_id {
                BencodeValue::String(BencodeString::String(tracker_id)) => Some(tracker_id.clone()),
                _ => return Err("tracker id key not found".to_string()),
            },
            None => None,
        };

        let complete = match value.get_value("complete") {
            Some(complete) => match complete {
                BencodeValue::Int(complete) => *complete,
                _ => return Err("complete key not found".to_string()),
            },
            None => return Err("complete key not found".to_string()),
        };

        let incomplete = match value.get_value("incomplete") {
            Some(incomplete) => match incomplete {
                BencodeValue::Int(incomplete) => *incomplete,
                _ => return Err("incomplete key not found".to_string()),
            },
            None => return Err("incomplete key not found".to_string()),
        };

        let Some(Ok(peers)) = value.get_value("peers").map(Tracker::parse_peers) else {
            return Err("peers key not found".to_string());
        };

        Ok(TrackerSuccessResponse {
            interval,
            min_interval,
            tracker_id,
            complete,
            incomplete,
            peers,
        })
    }

    fn to_tracker_response(parsed_value: &BencodeValue) -> Result<TrackerResponse, String> {
        let failure_response = parsed_value.get_value("failure reason").map(|value| {
            let failure_reason = match value {
                BencodeValue::String(BencodeString::String(reason)) => reason.clone(),
                _ => unreachable!(),
            };

            TrackerResponse::Failure(TrackerFailureResponse { failure_reason })
        });

        if let Some(failure_response) = failure_response {
            return Ok(failure_response);
        }

        let success_response = Tracker::parse_success_response(parsed_value)?;

        Ok(TrackerResponse::Success(success_response))
    }

    pub async fn get_announce(&self) -> Result<TrackerResponse, String> {
        let mut url = String::from(&self.metainfo.announce);

        let info_hash = self.get_info_hash().expect("Error getting info hash");
        url.push_str(format!("?info_hash={}", info_hash).as_str());
        url.push_str(format!("&peer_id={}", self.peer_id).as_str());
        url.push_str("&port=6881");

        println!("GET {}", &url);
        let response = reqwest::get(&url).await.map_err(|e| e.to_string())?;
        println!("{:#?}", response);

        let bytes = response
            .bytes()
            .await
            .map_err(|_| String::from("unable to parse response body"))?
            .to_vec();
        let (parsed_bencode, _) = BencodeValue::parse(&bytes).expect("Error parsing response");
        Tracker::to_tracker_response(&parsed_bencode)
    }

    fn get_peer_id() -> String {
        let mut peer_id = String::from("-RT0001-");
        let mut rng = rand::thread_rng();
        for _ in 0..(20 - peer_id.len()) {
            let random_char = (rng.gen_range(0..26) + 97) as u8 as char;
            peer_id.push(random_char);
        }
        peer_id
    }

    fn get_info_hash(&self) -> Result<String, String> {
        let info = match self.torrent_content.get_value("info") {
            Some(info) => info,
            None => return Err("info key not found".to_string()),
        };

        let info_bencoded = info.encode();

        let mut hasher = Sha1::new();
        hasher.update(info_bencoded);
        let result = hasher.finalize();
        let info_hash = url::form_urlencoded::byte_serialize(&result).collect::<String>();

        Ok(info_hash)
    }
}
