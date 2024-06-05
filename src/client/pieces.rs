use std::collections::HashSet;

use crate::metainfo::Info;

use super::bitfield::Bitfield;

pub const BLOCK_SIZE: u32 = 2 << 13; // 16KB

#[derive(Debug)]
pub struct Block {
    begin: u32,
    length: u32,
    data: Vec<u8>,
    requested: bool,
}

#[derive(Debug)]
pub struct Piece {
    index: usize,
    blocks: Vec<Block>,
    hash: Vec<u8>,
    completed: bool,
    peers: HashSet<Vec<u8>>,
}

#[derive(Debug)]
pub struct Pieces {
    pieces: Vec<Piece>,
    any_complete: bool,
}

impl Pieces {
    pub fn new(info_dict: &Info) -> Self {
        let (piece_hashes, piece_length, file_size) = match info_dict {
            Info::SingleFile(info) => (
                info.base_info.pieces.clone(),
                info.base_info.piece_length,
                info.length,
            ),
            Info::MultiFile(info) => (
                info.base_info.pieces.clone(),
                info.base_info.piece_length,
                info.files.iter().map(|f| f.length).sum(),
            ),
        };

        assert!(
            piece_length as u32 % BLOCK_SIZE == 0,
            "piece length must be a multiple of the block size"
        );

        let mut remaining_size = file_size as u32;
        let mut pieces = Vec::new();
        for (i, hash) in piece_hashes.iter().enumerate() {
            let mut blocks = Vec::new();
            let mut offset: u32 = 0;
            while offset < (piece_length as u32).min(remaining_size) {
                let length = if remaining_size < BLOCK_SIZE {
                    remaining_size
                } else {
                    BLOCK_SIZE
                };
                let block = Block {
                    begin: offset,
                    length,
                    data: Vec::new(),
                    requested: false,
                };
                blocks.push(block);

                remaining_size -= length;
                offset += length;
            }

            let piece = Piece {
                index: i,
                blocks,
                hash: hash.to_vec(),
                completed: false,
                peers: HashSet::new(),
            };
            pieces.push(piece);
        }

        Self {
            pieces,
            any_complete: false,
        }
    }

    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    pub fn to_bitfield(&self) -> Bitfield {
        let mut bitfield = Bitfield::new(self.len());
        for piece in &self.pieces {
            bitfield.set(piece.index, piece.completed).unwrap();
        }
        bitfield
    }

    fn get_rarest_noncompleted_piece(&self) -> &Piece {
        self.pieces
            .iter()
            .filter(|p| !p.completed && p.blocks.iter().any(|b| !b.requested))
            .min_by_key(|p| p.peers.len())
            .unwrap()
    }

    pub fn set_requested(&mut self, index: usize, begin: u32) {
        let piece = &mut self.pieces[index];

        let block_bucket: usize = begin.div_ceil(BLOCK_SIZE).try_into().unwrap();
        let block = &mut piece.blocks[block_bucket];
        block.requested = true;
    }

    pub fn set_block(&mut self, index: usize, begin: u32, data: Vec<u8>) {
        let piece = &mut self.pieces[index];

        let block_bucket: usize = begin.div_ceil(BLOCK_SIZE).try_into().unwrap();
        let block = &mut piece.blocks[block_bucket];
        block.data = data;
    }

    pub fn add_peer_count(&mut self, peer_id: &Vec<u8>, bitfield: &Bitfield) {
        for (i, bit) in bitfield.iter().enumerate() {
            if *bit {
                self.pieces[i].peers.insert(peer_id.clone());
            }
        }
    }

    pub fn add_peer_have(&mut self, peer_id: &Vec<u8>, i: usize) {
        self.pieces[i].peers.insert(peer_id.clone());
    }

    pub fn remove_peer_count(&mut self, peer_id: &Vec<u8>, bitfield: &Bitfield) {
        for (i, bit) in bitfield.iter().enumerate() {
            if *bit {
                self.pieces[i].peers.remove(peer_id);
            }
        }
    }

    pub fn get_block_to_download(&mut self) -> (usize, u32, u32) {
        let piece = if !self.any_complete {
            let pieces = self
                .pieces
                .iter()
                .filter(|p| !p.completed && p.blocks.iter().any(|b| !b.requested))
                .collect::<Vec<&Piece>>();
            pieces[rand::random::<usize>() % pieces.len()]
        } else {
            self.get_rarest_noncompleted_piece()
        };

        let block = piece
            .blocks
            .iter()
            .find(|b| !b.requested && b.data.is_empty())
            .unwrap();

        (piece.index, block.begin, block.length)
    }
}
