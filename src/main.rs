use std::{fs::File, io::Read};

use clap::Parser;
use rustorrent::{bencode::BencodeValue, client::Client, tracker::Tracker};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    file_path: String,
}

fn read_file(filename: &str) -> Result<Vec<u8>, std::io::Error> {
    let mut file = File::open(filename)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    Ok(contents)
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let file_content = match read_file(&args.file_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            return;
        }
    };

    let Ok((bencode_value, rest)) = BencodeValue::parse(&file_content) else {
        eprintln!("Error parsing bencode");
        return;
    };

    if rest.len() > 0 {
        eprintln!("Error parsing bencode: torrent file was not fully parsed");
        return;
    }

    let tracker = Tracker::new(bencode_value);
    let Ok(mut client) = Client::new(tracker) else {
        eprintln!("Error creating client");
        return;
    };

    client
        .connect_to_peers(1)
        .await
        .expect("Failed to connect to peers");
}
