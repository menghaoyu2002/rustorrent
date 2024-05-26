use core::fmt;
use std::{collections::BTreeMap, fmt::Display};

mod encoder;
mod parser;

#[derive(Debug, PartialEq)]
pub struct ParseError {
    pub value: String,
    pub message: String,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.message, self.value)
    }
}

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

impl Clone for BencodeValue {
    fn clone(&self) -> BencodeValue {
        match self {
            BencodeValue::String(BencodeString::String(s)) => {
                BencodeValue::String(BencodeString::String(s.clone()))
            }
            BencodeValue::String(BencodeString::Bytes(b)) => {
                BencodeValue::String(BencodeString::Bytes(b.clone()))
            }
            BencodeValue::Int(i) => BencodeValue::Int(*i),
            BencodeValue::List(l) => {
                let mut result = Vec::new();
                for item in l {
                    result.push(item.clone());
                }
                BencodeValue::List(result)
            }
            BencodeValue::Dict(d) => {
                let mut result = BTreeMap::new();
                for (k, v) in d {
                    result.insert(k.clone(), v.clone());
                }
                BencodeValue::Dict(result)
            }
        }
    }
}

impl BencodeValue {
    pub fn encode(&self) -> Vec<u8> {
        encoder::encode_bencode(self)
    }

    pub fn parse(data: &Vec<u8>) -> Result<(BencodeValue, Vec<u8>), ParseError> {
        parser::parse_bencode(data)
    }

    pub fn get_value(&self, key: &str) -> Option<&BencodeValue> {
        match self {
            BencodeValue::Dict(dict) => dict.get(key),
            _ => None,
        }
    }
}
