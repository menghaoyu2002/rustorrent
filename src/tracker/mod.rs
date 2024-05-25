use rand::Rng;
use sha1::{Digest, Sha1};

use crate::bencode::{BencodeValue, Metainfo};

pub struct Tracker {
    torrent_content: BencodeValue,
    metainfo: Metainfo,
    peer_id: String,
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

    pub async fn get_announce(&self) -> Result<Vec<u8>, reqwest::Error> {
        let mut url = String::from(&self.metainfo.announce);

        let info_hash = self.get_info_hash().expect("Error getting info hash");
        url.push_str(format!("?info_hash={}", info_hash).as_str());
        url.push_str(format!("&&peer_id={}", self.peer_id).as_str());
        url.push_str("&port=6881");

        println!("GET {}", &url);
        let response = reqwest::get(&url).await?;
        println!("{:#?}", response);
        Ok(response.bytes().await?.to_vec())
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
