use std::collections::HashMap;

pub mod parser;

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
    Dict(HashMap<String, BencodeValue>),
}
