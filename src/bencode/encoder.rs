use std::collections::BTreeMap;

use super::{BencodeString, BencodeValue};

pub fn encode_bencode(value: &BencodeValue) -> Vec<u8> {
    match value {
        BencodeValue::String(BencodeString::Bytes(bytes)) => encode_bytes(bytes),
        BencodeValue::String(BencodeString::String(text)) => encode_string(text),
        BencodeValue::Int(int) => encode_int(int),
        BencodeValue::List(list) => encode_list(list),
        BencodeValue::Dict(dict) => encode_dict(dict),
    }
}

fn encode_string(text: &str) -> Vec<u8> {
    format!("{}:{}", text.len(), text).into_bytes()
}

fn encode_bytes(bytes: &Vec<u8>) -> Vec<u8> {
    let mut result = format!("{}:", bytes.len()).into_bytes();
    result.extend_from_slice(&bytes);
    result
}

fn encode_int(int: &i64) -> Vec<u8> {
    format!("i{}e", int).into_bytes()
}

fn encode_list(list: &Vec<BencodeValue>) -> Vec<u8> {
    let mut result = Vec::new();
    result.push(b'l');
    for item in list {
        result.extend_from_slice(&encode_bencode(item));
    }
    result.push(b'e');
    result
}

fn encode_dict(dict: &BTreeMap<String, BencodeValue>) -> Vec<u8> {
    let mut result = Vec::new();
    result.push(b'd');

    for (key, value) in dict {
        result.extend_from_slice(&encode_string(key));
        result.extend_from_slice(&encode_bencode(value));
    }

    result.push(b'e');
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bencode::parser::parse_bencode;

    #[test]
    fn test_encode_bytes() {
        let input: Vec<u8> = "hello".bytes().collect();
        let expected = "5:hello".as_bytes();
        assert_eq!(encode_bytes(&input), expected);
    }

    #[test]
    fn test_encode_string() {
        let input = "hello";
        let expected = "5:hello".as_bytes();
        assert_eq!(encode_string(input), expected);
    }

    #[test]
    fn test_encode_int() {
        let input = 123;
        let expected = "i123e".as_bytes();
        assert_eq!(encode_int(&input), expected);
    }

    #[test]
    fn test_encode_list() {
        let input = vec![
            BencodeValue::Int(123),
            BencodeValue::String(BencodeString::String("hello".to_string())),
        ];
        let expected = "li123e5:helloe".as_bytes();
        assert_eq!(encode_list(&input), expected);
    }

    #[test]
    fn test_encode_dict() {
        let mut input = BTreeMap::new();
        input.insert("key".to_string(), BencodeValue::Int(123));
        input.insert(
            "key2".to_string(),
            BencodeValue::String(BencodeString::String("hello".to_string())),
        );

        let expected = "d3:keyi123e4:key25:helloe".as_bytes();
        assert_eq!(encode_dict(&input), expected);
    }

    #[test]
    fn test_encode_bencode() {
        let input = "d3:keyd3:keyd3:key5:valuee4:listli123e5:Hello5:Worldeee"
            .as_bytes()
            .to_vec();
        let (parsed, _) = parse_bencode(&input).unwrap();
        assert_eq!(encode_bencode(&parsed), input);
    }
}
