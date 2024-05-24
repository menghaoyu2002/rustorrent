use std::{fs::File, io::Read};

use clap::Parser;
use rustorrent::bencode::parser::parse_bencode;

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

fn main() {
    let args = Args::parse();
    let file_content = match read_file(&args.file_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            return;
        }
    };

    let Ok((parsed_value, rest)) = parse_bencode(&file_content) else {
        eprintln!("Error parsing bencode");
        return;
    };
    assert!(rest.is_empty(), "Torrent file is not fully parsed");

    let Ok(metainfo) = parsed_value.to_metainfo() else {
        eprintln!("Error parsing metainfo");
        return;
    };

    println!("{:#?}", metainfo);
}
