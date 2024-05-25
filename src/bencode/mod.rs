use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

mod encoder;
mod parser;

#[derive(Debug, PartialEq)]
pub enum BencodeString {
    String(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, PartialEq)]
pub enum BencodeValue {
    String(BencodeString),
    Int(i64),
    List(Vec<BencodeValue>),
    Dict(BTreeMap<String, BencodeValue>),
}

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
    pub info: Info,
    pub announce: String,
    pub announce_list: Option<Vec<Vec<String>>>,
    pub creation_date: Option<DateTime<Utc>>,
    pub comment: Option<String>,
    pub created_by: Option<String>,
    pub encoding: Option<String>,
}

impl Metainfo {
    fn dict_to_base_info(dict: &BTreeMap<String, BencodeValue>) -> Result<BaseInfo, String> {
        let pieces = match dict.get("pieces") {
            Some(BencodeValue::String(BencodeString::Bytes(b))) => b.clone(),
            _ => return Err("Invalid 'pieces' attribute".to_string()),
        };

        let piece_length = match dict.get("piece length") {
            Some(BencodeValue::Int(i)) => *i,
            _ => return Err("Invalid 'piece length' attribute".to_string()),
        };

        let private = dict
            .get("private")
            .map(|v| match v {
                BencodeValue::Int(i) => Ok(*i),
                _ => Err("Invalid 'private' attribute".to_string()),
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
    ) -> Result<SingleFileInfo, String> {
        let base_info = Metainfo::dict_to_base_info(dict)?;

        let name = match dict.get("name") {
            Some(BencodeValue::String(BencodeString::String(s))) => s.clone(),
            _ => return Err("Invalid 'name' attribute".to_string()),
        };

        let length = match dict.get("length") {
            Some(BencodeValue::Int(i)) => *i,
            _ => return Err("Invalid 'length' attribute".to_string()),
        };

        let md5sum = dict
            .get("md5sum")
            .map(|v| match v {
                BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                _ => Err("Invalid 'md5sum' attribute".to_string()),
            })
            .transpose()?;

        Ok(SingleFileInfo {
            base_info,
            name,
            length,
            md5sum,
        })
    }

    fn dict_to_multiple_file_info(
        dict: &BTreeMap<String, BencodeValue>,
    ) -> Result<MultiFileInfo, String> {
        let base_info = Metainfo::dict_to_base_info(dict)?;

        let name = match dict.get("name") {
            Some(BencodeValue::String(BencodeString::String(s))) => s.clone(),
            _ => return Err("Invalid 'name' attribute".to_string()),
        };

        let files = match dict.get("files") {
            Some(BencodeValue::List(v)) => {
                let mut result = Vec::new();
                for item in v {
                    match item {
                        BencodeValue::Dict(file_dict) => {
                            let path = match file_dict.get("path") {
                                Some(BencodeValue::List(path_list)) => {
                                    let mut result = Vec::new();
                                    for path_item in path_list {
                                        match path_item {
                                            BencodeValue::String(BencodeString::String(s)) => {
                                                result.push(s.clone());
                                            }
                                            _ => return Err("Invalid 'path' attribute".to_string()),
                                        }
                                    }
                                    result
                                }
                                _ => return Err("Invalid 'path' attribute".to_string()),
                            };

                            let length = match file_dict.get("length") {
                                Some(BencodeValue::Int(i)) => *i,
                                _ => return Err("Invalid 'length' attribute".to_string()),
                            };

                            let md5sum = file_dict
                                .get("md5sum")
                                .map(|v| match v {
                                    BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                                    _ => Err("Invalid 'md5sum' attribute".to_string()),
                                })
                                .transpose()?;

                            result.push(FileData {
                                path,
                                length,
                                md5sum,
                            });
                        }
                        _ => return Err("Invalid 'files' attribute".to_string()),
                    }
                }
                result
            }
            _ => return Err("Invalid 'files' attribute".to_string()),
        };

        Ok(MultiFileInfo {
            base_info,
            name,
            files,
        })
    }

    fn dict_to_info(dict: &BTreeMap<String, BencodeValue>) -> Result<Info, String> {
        match dict.get("files") {
            Some(BencodeValue::List(_)) => {
                let info = Metainfo::dict_to_multiple_file_info(dict)?;
                Ok(Info::MultiFile(info))
            }
            None => {
                let info = Metainfo::dict_to_single_file_info(dict)?;
                Ok(Info::SingleFile(info))
            }
            _ => Err("Invalid info".to_string()),
        }
    }

    fn convert_announce_list(value: &BencodeValue) -> Result<Vec<Vec<String>>, String> {
        match value {
            BencodeValue::List(list) => {
                let mut result = Vec::new();
                for item in list {
                    match item {
                        BencodeValue::List(inner_list) => {
                            let mut inner_result = Vec::new();
                            for inner_item in inner_list {
                                match inner_item {
                                    BencodeValue::String(BencodeString::String(s)) => {
                                        inner_result.push(s.clone());
                                    }
                                    _ => return Err("Invalid announce list".to_string()),
                                }
                            }
                            result.push(inner_result);
                        }
                        _ => return Err("Invalid announce list".to_string()),
                    }
                }
                Ok(result)
            }
            _ => Err("Invalid announce list".to_string()),
        }
    }

    fn dict_to_metainfo(dict: &BTreeMap<String, BencodeValue>) -> Result<Metainfo, String> {
        let announce = match dict.get("announce") {
            Some(BencodeValue::String(BencodeString::String(s))) => s.clone(),
            _ => return Err("Invalid 'announce' attribute".to_string()),
        };

        let creation_date = dict
            .get("creation date")
            .map(|v| match v {
                BencodeValue::Int(i) => DateTime::from_timestamp(*i, 0)
                    .ok_or("Invalid 'creation date' attribute".to_string()),

                _ => return Err("Invalid 'creation date' attribute".to_string()),
            })
            .transpose()?;

        let comment = dict
            .get("comment")
            .map(|v| match v {
                BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                _ => Err("Invalid 'comment' attribute"),
            })
            .transpose()?;

        let created_by = dict
            .get("created by")
            .map(|v| match v {
                BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                _ => Err("Invalid 'created by' attribute"),
            })
            .transpose()?;

        let encoding = dict
            .get("encoding")
            .map(|v| match v {
                BencodeValue::String(BencodeString::String(s)) => Ok(s.clone()),
                _ => Err("Invalid encoding"),
            })
            .transpose()?;

        let info = match dict.get("info") {
            Some(BencodeValue::Dict(info_dict)) => Metainfo::dict_to_info(info_dict),
            _ => Err("Invalid info".to_string()),
        }?;

        let announce_list = dict
            .get("announce-list")
            .map(|v| Metainfo::convert_announce_list(v))
            .transpose()?;

        Ok(Metainfo {
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

impl BencodeValue {
    pub fn encode(&self) -> Vec<u8> {
        encoder::encode_bencode(self)
    }

    pub fn parse(data: &Vec<u8>) -> Result<(BencodeValue, Vec<u8>), String> {
        parser::parse_bencode(data)
    }

    pub fn to_metainfo(&self) -> Result<Metainfo, String> {
        match self {
            BencodeValue::Dict(dict) => Metainfo::dict_to_metainfo(&dict),
            _ => Err("Invalid metainfo".to_string()),
        }
    }

    pub fn get_value(&self, key: &str) -> Option<&BencodeValue> {
        match self {
            BencodeValue::Dict(dict) => dict.get(key),
            _ => None,
        }
    }
}
