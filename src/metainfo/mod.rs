use std::{collections::BTreeMap, fmt::Debug};

use chrono::{DateTime, Utc};
use sha1::{Digest, Sha1};

use crate::bencode::{BencodeString, BencodeValue};

#[derive(Debug, PartialEq)]
pub struct BaseInfo {
    // shared by both single and multi file mode
    pub pieces: Vec<u8>,
    pub piece_length: i64,
    pub private: Option<i64>,
}

#[derive(Debug)]
pub struct SingleFileInfo {
    pub base_info: BaseInfo,
    pub name: String,
    pub length: i64,
    pub md5sum: Option<String>,
}

#[derive(Debug)]
pub struct FileData {
    pub path: Vec<String>,
    pub length: i64,
    pub md5sum: Option<String>,
}

#[derive(Debug)]
pub struct MultiFileInfo {
    pub base_info: BaseInfo,
    pub name: String,
    pub files: Vec<FileData>,
}

#[derive(Debug)]
pub enum Info {
    SingleFile(SingleFileInfo),
    MultiFile(MultiFileInfo),
}

#[derive(Debug)]
pub struct Metainfo {
    torrent_content: BencodeValue,

    pub info: Info,
    pub announce: String,
    pub announce_list: Option<Vec<Vec<String>>>,
    pub creation_date: Option<DateTime<Utc>>,
    pub comment: Option<String>,
    pub created_by: Option<String>,
    pub encoding: Option<String>,
}

pub struct AttributeError {
    pub content: BencodeValue,
    pub attribute: String,
}

pub enum MetaInfoError {
    InvalidAttribute(AttributeError),
    InvalidBencodeValue,
}

impl Debug for MetaInfoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetaInfoError::InvalidAttribute(e) => {
                write!(f, "InvalidAttribute: {:?} {:?}", e.content, e.attribute)
            }
            MetaInfoError::InvalidBencodeValue => write!(f, "InvalidBencodeValue"),
        }
    }
}

impl Metainfo {
    pub fn new(bencode_value: BencodeValue) -> Result<Metainfo, MetaInfoError> {
        match bencode_value.clone() {
            BencodeValue::Dict(dict) => Metainfo::dict_to_metainfo(bencode_value, &dict),
            _ => Err(MetaInfoError::InvalidBencodeValue),
        }
    }

    pub fn get_info_hash(&self) -> Result<Vec<u8>, MetaInfoError> {
        let info = match self.torrent_content.get_value("info") {
            Some(info) => info,
            None => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: self.torrent_content.clone(),
                    attribute: "info".to_string(),
                }))
            }
        };

        let info_bencoded = info.encode();

        let mut hasher = Sha1::new();
        hasher.update(info_bencoded);
        let result = hasher.finalize();

        Ok(result.to_vec())
    }

    fn dict_to_base_info(dict: &BTreeMap<String, BencodeValue>) -> Result<BaseInfo, MetaInfoError> {
        let pieces = match dict.get("pieces") {
            Some(BencodeValue::String(BencodeString::Bytes(b))) => b.clone(),
            _ => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: BencodeValue::Dict(dict.clone()),
                    attribute: "pieces".to_string(),
                }))
            }
        };

        let piece_length = match dict.get("piece length") {
            Some(BencodeValue::Int(i)) => *i,
            _ => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: BencodeValue::Dict(dict.clone()),
                    attribute: "piece length".to_string(),
                }))
            }
        };

        let private = dict
            .get("private")
            .map(|v| match v {
                BencodeValue::Int(i) => Ok(*i),
                _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: BencodeValue::Dict(dict.clone()),
                    attribute: "private".to_string(),
                })),
            })
            .transpose()?;

        Ok(BaseInfo {
            pieces,
            piece_length,
            private,
        })
    }

    fn dict_to_single_file_info(
        dict: &BTreeMap<String, BencodeValue>,
    ) -> Result<SingleFileInfo, MetaInfoError> {
        let base_info = Metainfo::dict_to_base_info(dict)?;

        let name = match dict.get("name") {
            Some(BencodeValue::String(BencodeString::String(s))) => s.clone(),
            _ => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: BencodeValue::Dict(dict.clone()),
                    attribute: "name".to_string(),
                }))
            }
        };

        let length = match dict.get("length") {
            Some(BencodeValue::Int(i)) => *i,
            _ => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: BencodeValue::Dict(dict.clone()),
                    attribute: "length".to_string(),
                }))
            }
        };

        let md5sum = dict
            .get("md5sum")
            .map(|v| match v {
                BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: BencodeValue::Dict(dict.clone()),
                    attribute: "md5sum".to_string(),
                })),
            })
            .transpose()?;

        Ok(SingleFileInfo {
            base_info,
            name,
            length,
            md5sum,
        })
    }

    fn parse_file(file: &BencodeValue) -> Result<FileData, MetaInfoError> {
        match file {
            BencodeValue::Dict(file_dict) => {
                let path = match file_dict.get("path") {
                    Some(BencodeValue::List(path_list)) => path_list
                        .iter()
                        .map(|path_item| match path_item {
                            BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                            _ => {
                                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                                    content: BencodeValue::Dict(file_dict.clone()),
                                    attribute: "path".to_string(),
                                }))
                            }
                        })
                        .collect::<Result<Vec<String>, MetaInfoError>>(),

                    _ => {
                        return Err(MetaInfoError::InvalidAttribute(AttributeError {
                            content: BencodeValue::Dict(file_dict.clone()),
                            attribute: "path".to_string(),
                        }))
                    }
                }?;

                let length = match file_dict.get("length") {
                    Some(BencodeValue::Int(i)) => *i,
                    _ => {
                        return Err(MetaInfoError::InvalidAttribute(AttributeError {
                            content: BencodeValue::Dict(file_dict.clone()),
                            attribute: "length".to_string(),
                        }))
                    }
                };

                let md5sum = file_dict
                    .get("md5sum")
                    .map(|v| match v {
                        BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                        _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                            content: BencodeValue::Dict(file_dict.clone()),
                            attribute: "md5sum".to_string(),
                        })),
                    })
                    .transpose()?;

                Ok(FileData {
                    path,
                    length,
                    md5sum,
                })
            }
            _ => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: file.clone(),
                    attribute: "file".to_string(),
                }))
            }
        }
    }

    fn dict_to_multiple_file_info(
        dict: &BTreeMap<String, BencodeValue>,
    ) -> Result<MultiFileInfo, MetaInfoError> {
        let base_info = Metainfo::dict_to_base_info(dict)?;

        let name = match dict.get("name") {
            Some(BencodeValue::String(BencodeString::String(s))) => s.clone(),
            _ => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: BencodeValue::Dict(dict.clone()),
                    attribute: "name".to_string(),
                }))
            }
        };

        let files = match dict.get("files") {
            Some(BencodeValue::List(v)) => v
                .iter()
                .map(Metainfo::parse_file)
                .collect::<Result<Vec<FileData>, MetaInfoError>>(),
            _ => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: BencodeValue::Dict(dict.clone()),
                    attribute: "files".to_string(),
                }))
            }
        }?;

        Ok(MultiFileInfo {
            base_info,
            name,
            files,
        })
    }

    fn dict_to_info(dict: &BTreeMap<String, BencodeValue>) -> Result<Info, MetaInfoError> {
        match dict.get("files") {
            Some(BencodeValue::List(_)) => {
                let info = Metainfo::dict_to_multiple_file_info(dict)?;
                Ok(Info::MultiFile(info))
            }
            None => {
                let info = Metainfo::dict_to_single_file_info(dict)?;
                Ok(Info::SingleFile(info))
            }
            _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                content: BencodeValue::Dict(dict.clone()),
                attribute: "files".to_string(),
            })),
        }
    }

    fn convert_announce_list(value: &BencodeValue) -> Result<Vec<Vec<String>>, MetaInfoError> {
        match value {
            BencodeValue::List(list) => list
                .iter()
                .map(|item| match item {
                    BencodeValue::List(inner_list) => {
                        let mut inner_result = Vec::new();
                        for inner_item in inner_list {
                            match inner_item {
                                BencodeValue::String(BencodeString::String(s)) => {
                                    inner_result.push(s.clone());
                                }
                                _ => {
                                    return Err(MetaInfoError::InvalidAttribute(AttributeError {
                                        content: item.clone(),
                                        attribute: "announce-list".to_string(),
                                    }))
                                }
                            }
                        }
                        Ok(inner_result)
                    }
                    _ => {
                        return Err(MetaInfoError::InvalidAttribute(AttributeError {
                            content: item.clone(),
                            attribute: "announce-list".to_string(),
                        }))
                    }
                })
                .collect::<Result<Vec<Vec<String>>, MetaInfoError>>(),
            _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                content: value.clone(),
                attribute: "announce-list".to_string(),
            })),
        }
    }

    fn dict_to_metainfo(
        bencode_value: BencodeValue,
        dict: &BTreeMap<String, BencodeValue>,
    ) -> Result<Metainfo, MetaInfoError> {
        let announce = match dict.get("announce") {
            Some(BencodeValue::String(BencodeString::String(s))) => s.clone(),
            _ => {
                return Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: bencode_value.clone(),
                    attribute: "announce".to_string(),
                }))
            }
        };

        let creation_date = dict
            .get("creation date")
            .map(|v| match v {
                BencodeValue::Int(i) => DateTime::from_timestamp(*i, 0).ok_or(
                    MetaInfoError::InvalidAttribute(AttributeError {
                        content: bencode_value.clone(),
                        attribute: "creation date".to_string(),
                    }),
                ),

                _ => {
                    return Err(MetaInfoError::InvalidAttribute(AttributeError {
                        content: bencode_value.clone(),
                        attribute: "creation date".to_string(),
                    }))
                }
            })
            .transpose()?;

        let comment = dict
            .get("comment")
            .map(|v| match v {
                BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: bencode_value.clone(),
                    attribute: "comment".to_string(),
                })),
            })
            .transpose()?;

        let created_by = dict
            .get("created by")
            .map(|v| match v {
                BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: bencode_value.clone(),
                    attribute: "created by".to_string(),
                })),
            })
            .transpose()?;

        let encoding = dict
            .get("encoding")
            .map(|v| match v {
                BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                    content: bencode_value.clone(),
                    attribute: "encoding".to_string(),
                })),
            })
            .transpose()?;

        let info = match dict.get("info") {
            Some(BencodeValue::Dict(info_dict)) => Metainfo::dict_to_info(info_dict),
            _ => Err(MetaInfoError::InvalidAttribute(AttributeError {
                content: bencode_value.clone(),
                attribute: "info".to_string(),
            })),
        }?;

        let announce_list = dict
            .get("announce-list")
            .map(|v| Metainfo::convert_announce_list(v))
            .transpose()?;

        Ok(Metainfo {
            torrent_content: bencode_value,
            info,
            announce,
            announce_list,
            creation_date,
            comment,
            created_by,
            encoding,
        })
    }
}
