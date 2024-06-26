use std::{
    fs::{create_dir_all, File, OpenOptions},
    os::unix::fs::FileExt,
};

use sha1::Digest;

use crate::metainfo::Info;

#[derive(Debug)]
pub struct FileManager {
    piece_length: u64,
    files: Vec<(File, u64)>,
}

impl FileManager {
    pub fn new(output_dir: String, info_dict: &Info) -> Self {
        create_dir_all(&output_dir).unwrap();
        match info_dict {
            Info::SingleFile(info) => {
                let file_path = format!("{}/{}", output_dir, info.name);
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(file_path)
                    .unwrap();
                FileManager {
                    piece_length: info.base_info.piece_length as u64,
                    files: vec![(file, info.length)],
                }
            }
            Info::MultiFile(info) => {
                let mut files = Vec::new();
                for file_info in &info.files {
                    let file_path = format!("{}/{}", output_dir, file_info.path.join("/"));
                    let file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .open(file_path)
                        .unwrap();
                    files.push((file, file_info.length));
                }
                FileManager {
                    piece_length: info.base_info.piece_length as u64,
                    files,
                }
            }
        }
    }

    pub fn save_block(&mut self, piece_index: usize, begin: u32, data: Vec<u8>) {
        let byte_offset = self.piece_length * piece_index as u64 + begin as u64;
        let mut accumulated_size = 0;
        for (file, file_size) in &mut self.files {
            if byte_offset < accumulated_size + *file_size {
                file.write_at(&data, byte_offset - accumulated_size)
                    .unwrap();
                break;
            }
            accumulated_size += *file_size;
        }
    }

    pub fn verify_piece(&self, piece_index: usize, hash: &[u8]) -> bool {
        let offset = self.piece_length * piece_index as u64;
        let mut file_index = 0;
        let mut accumulated_size = 0;
        while offset >= self.files[file_index].1 + accumulated_size {
            accumulated_size += self.files[file_index].1;
            file_index += 1;
        }
        let file = &self.files[file_index].0;
        let mut buf = vec![0; self.piece_length as usize];
        file.read_at(&mut buf, offset).unwrap();

        let mut hasher = sha1::Sha1::new();
        hasher.update(&buf);
        let result = hasher.finalize().to_vec();
        hash == result.as_slice()
    }
}
