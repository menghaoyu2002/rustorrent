use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
};

use rand::Rng;

use crate::{
    bencode::{BencodeString, BencodeValue},
    metainfo::Metainfo,
};

pub struct InvalidResponseError {
    pub url: String,
    pub status: reqwest::StatusCode,
    pub message: String,
}

pub enum TrackerError {
    InvalidMetainfo,
    InvalidInfoHash,
    GetPeersFailure(String),
    GetAccounceError(String),
    InvalidResponse(InvalidResponseError),
    ResponseParseError(String),
}

#[derive(Debug)]
pub struct Tracker {
    metainfo: Metainfo,
    peer_id: String,
}

#[derive(Debug)]
pub struct Peer {
    pub addr: SocketAddr,
    pub peer_id: Option<String>,
}

pub type Peers = Vec<Peer>;

#[derive(Debug)]
pub struct TrackerSuccessResponse {
    pub interval: i64,
    pub min_interval: Option<i64>,
    pub tracker_id: Option<String>,
    pub complete: i64,
    pub incomplete: i64,
    pub peers: Peers,
}

#[derive(Debug)]
pub struct TrackerFailureResponse {
    pub failure_reason: String,
}

#[derive(Debug)]
pub enum TrackerResponse {
    Success(TrackerSuccessResponse),
    Failure(TrackerFailureResponse),
}

impl Tracker {
    pub fn new(torrent_content: BencodeValue) -> Self {
        let metainfo = Metainfo::new(torrent_content).expect("Invalid metainfo");

        Self {
            metainfo,
            peer_id: Tracker::get_peer_id(),
        }
    }

    pub fn get_metainfo(&self) -> &Metainfo {
        &self.metainfo
    }

    pub fn peer_id(&self) -> String {
        self.peer_id.to_string()
    }

    pub async fn get_peers(&self) -> Result<Peers, TrackerError> {
        let response = self.get_announce().await?;
        let peers = match response {
            TrackerResponse::Success(success_response) => success_response.peers,
            TrackerResponse::Failure(failure_response) => {
                return Err(TrackerError::GetPeersFailure(
                    failure_response.failure_reason,
                ))
            }
        };

        Ok(peers)
    }

    fn parse_peers(value: &BencodeValue) -> Result<Peers, TrackerError> {
        match value {
            BencodeValue::String(BencodeString::Bytes(raw_peers)) => {
                let mut peers = Vec::new();
                for peer in raw_peers.chunks(6) {
                    let port = u16::from(peer[4]) << 8 | u16::from(peer[5]);
                    peers.push(Peer {
                        addr: SocketAddr::new(
                            IpAddr::V4(Ipv4Addr::new(peer[0], peer[1], peer[2], peer[3])),
                            port,
                        ),
                        peer_id: None,
                    });
                }
                return Ok(peers);
            }
            BencodeValue::List(peers) => {
                let mut parsed_peers = Vec::new();
                for peer in peers {
                    match peer {
                        BencodeValue::Dict(dict) => {
                            let ip = match dict.get("ip") {
                                Some(BencodeValue::String(BencodeString::String(ip))) => ip.clone(),
                                _ => {
                                    return Err(TrackerError::GetPeersFailure(
                                        "ip key not found".to_string(),
                                    ))
                                }
                            };

                            let port = match dict.get("port") {
                                Some(BencodeValue::Int(port)) => *port,
                                _ => {
                                    return Err(TrackerError::GetPeersFailure(
                                        "port key not found".to_string(),
                                    ))
                                }
                            };

                            let peer_id = dict
                                .get("peer id")
                                .map(|peer_id| match peer_id {
                                    BencodeValue::String(BencodeString::String(peer_id)) => {
                                        Some(peer_id.clone())
                                    }
                                    _ => None,
                                })
                                .flatten();

                            parsed_peers.push(Peer {
                                peer_id,
                                addr: SocketAddr::new(
                                    IpAddr::from_str(&ip).map_err(|e| {
                                        TrackerError::GetPeersFailure(e.to_string())
                                    })?,
                                    port as u16,
                                ),
                            });
                        }
                        _ => {
                            return Err(TrackerError::GetPeersFailure("invalid peers".to_string()))
                        }
                    }
                }
                return Ok(parsed_peers);
            }
            _ => return Err(TrackerError::GetPeersFailure("invalid peers".to_string())),
        }
    }

    fn parse_success_response(
        value: &BencodeValue,
    ) -> Result<TrackerSuccessResponse, TrackerError> {
        let interval = match value.get_value("interval") {
            Some(interval) => match interval {
                BencodeValue::Int(interval) => *interval,
                _ => unreachable!(),
            },
            None => {
                return Err(TrackerError::ResponseParseError(
                    "interval key not found".to_string(),
                ))
            }
        };

        let min_interval = match value.get_value("min interval") {
            Some(min_interval) => match min_interval {
                BencodeValue::Int(min_interval) => Some(*min_interval),
                _ => {
                    return Err(TrackerError::ResponseParseError(
                        "min interval key not found".to_string(),
                    ))
                }
            },
            None => None,
        };

        let tracker_id = match value.get_value("tracker id") {
            Some(tracker_id) => match tracker_id {
                BencodeValue::String(BencodeString::String(tracker_id)) => Some(tracker_id.clone()),
                _ => {
                    return Err(TrackerError::ResponseParseError(
                        "tracker id key not found".to_string(),
                    ))
                }
            },
            None => None,
        };

        let complete = match value.get_value("complete") {
            Some(complete) => match complete {
                BencodeValue::Int(complete) => *complete,
                _ => {
                    return Err(TrackerError::ResponseParseError(
                        "complete key not found".to_string(),
                    ))
                }
            },
            None => {
                return Err(TrackerError::ResponseParseError(
                    "complete key not found".to_string(),
                ))
            }
        };

        let incomplete = match value.get_value("incomplete") {
            Some(incomplete) => match incomplete {
                BencodeValue::Int(incomplete) => *incomplete,
                _ => {
                    return Err(TrackerError::ResponseParseError(
                        "incomplete key not found".to_string(),
                    ))
                }
            },
            None => {
                return Err(TrackerError::ResponseParseError(
                    "incomplete key not found".to_string(),
                ))
            }
        };

        let Some(Ok(peers)) = value.get_value("peers").map(Tracker::parse_peers) else {
            return Err(TrackerError::ResponseParseError(
                "peers key not found".to_string(),
            ));
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

    fn to_tracker_response(parsed_value: &BencodeValue) -> Result<TrackerResponse, TrackerError> {
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

    pub async fn get_announce(&self) -> Result<TrackerResponse, TrackerError> {
        let mut url = String::from(&self.metainfo.announce);

        let info_hash = self
            .metainfo
            .get_info_hash()
            .expect("Error getting info hash");

        let url_encoded_info_hash =
            url::form_urlencoded::byte_serialize(&info_hash).collect::<String>();

        url.push_str(format!("?info_hash={}", url_encoded_info_hash).as_str());
        url.push_str(format!("&peer_id={}", self.peer_id).as_str());
        url.push_str("&port=6881");

        println!("GET {}", &url);
        let response = reqwest::get(&url)
            .await
            .map_err(|e| TrackerError::GetAccounceError(e.to_string()))?;
        println!("GET {}", response.status());

        let bytes = response
            .bytes()
            .await
            .map_err(|e| {
                TrackerError::InvalidResponse(InvalidResponseError {
                    url,
                    status: e
                        .status()
                        .unwrap_or(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
                    message: e.to_string(),
                })
            })?
            .to_vec();

        let (parsed_bencode, _) =
            BencodeValue::parse(&bytes).map_err(|e| TrackerError::ResponseParseError(e.message))?;

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
}
