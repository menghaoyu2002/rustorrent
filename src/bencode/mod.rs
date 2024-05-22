use std::collections::HashMap;

pub mod parser;

#[derive(Debug, PartialEq)]
pub enum BencodeValue {
    String(String),
    Int(i64),
    List(Vec<BencodeValue>),
    Dict(HashMap<String, BencodeValue>),
}
