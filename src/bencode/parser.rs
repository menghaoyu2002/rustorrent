use std::collections::BTreeMap;

use super::{BencodeString, BencodeValue};

fn parse_string(input: &Vec<u8>) -> Result<(BencodeString, Vec<u8>), String> {
    let mut length = 0;
    let mut i = 0;
    while let Some(char) = input.get(i) {
        if *char == b':' {
            break;
        }

        if char.is_ascii_digit() {
            length = length * 10 + (char - b'0') as usize;
        } else {
            return Err(format!(
                "Invalid Bencode String '{}'",
                String::from_utf8_lossy(input)
            ));
        }

        i += 1;
    }

    if i + 1 + length > input.len() {
        return Err(format!(
            "Invalid Bencode String '{}': Length exceeds input length",
            String::from_utf8_lossy(input)
        ));
    }

    let str_segment = &input[i + 1..i + 1 + length];
    let str = match std::str::from_utf8(str_segment) {
        Ok(s) => BencodeString::String(s.to_string()),
        Err(_) => BencodeString::Bytes(str_segment.to_vec()),
    };

    Ok((str, input[i + 1 + length..].to_vec()))
}

fn parse_int(input: &Vec<u8>) -> Result<(i64, Vec<u8>), String> {
    if input.get(0) != Some(&b'i') {
        return Err(format!(
            "Invalid Bencode Integer '{}'",
            String::from_utf8_lossy(input)
        ));
    }

    let mut i = 1;
    let mut int: i64 = 0;
    let is_negative = if input.get(i) == Some(&b'-') {
        i += 1;
        true
    } else {
        false
    };

    let starting_index = i;
    let mut starts_with_zero = false;
    while let Some(char) = input.get(i) {
        if *char == b'e' {
            break;
        }

        if starts_with_zero && i != starting_index {
            return Err(format!(
                "Invalid Bencode Integer '{}', cannot be prefixed with 0",
                String::from_utf8_lossy(input)
            ));
        }

        if *char == b'0' && i == starting_index {
            starts_with_zero = true;
        }

        if char.is_ascii_digit() {
            int = int * 10 + (*char - b'0') as i64;
        } else {
            return Err(format!(
                "Could not parse Bencode integer '{}'",
                String::from_utf8_lossy(input)
            ));
        }

        i += 1;
    }

    if is_negative && int == 0 {
        return Err(format!(
            "Invalid Bencode Integer '{}'",
            String::from_utf8_lossy(input)
        ));
    }

    if is_negative {
        int = -int;
    }

    Ok((int, input[i + 1..].to_vec()))
}

fn parse_list(input: &Vec<u8>) -> Result<(Vec<BencodeValue>, Vec<u8>), String> {
    if input.get(0) != Some(&b'l') {
        return Err(format!(
            "Invalid Bencode List '{}'",
            String::from_utf8_lossy(input)
        ));
    }

    let mut rest = input[1..].to_vec();
    let mut list = Vec::new();
    while let Some(char) = rest.get(0) {
        if *char == b'e' {
            return Ok((list, rest[1..].to_vec()));
        }

        let (value, updated_rest) = parse_bencode(&rest)?;
        rest = updated_rest;
        list.push(value);
    }

    Err(format!(
        "Invalid Bencode List '{}'",
        String::from_utf8_lossy(input)
    ))
}

fn parse_dict(input: &Vec<u8>) -> Result<(BTreeMap<String, BencodeValue>, Vec<u8>), String> {
    if input.get(0) != Some(&b'd') {
        return Err(format!(
            "Invalid Bencode Dict '{}'",
            String::from_utf8_lossy(input)
        ));
    }

    let mut rest = input[1..].to_vec();
    let mut dict = BTreeMap::new();
    while let Some(char) = rest.get(0) {
        if *char == b'e' {
            return Ok((dict, rest[1..].to_vec()));
        }

        let (key, key_rest) = parse_string(&rest)?;
        let (value, updated_rest) = parse_bencode(&key_rest)?;
        match key {
            BencodeString::String(s) => dict.insert(s, value),
            BencodeString::Bytes(b) => dict.insert(String::from_utf8_lossy(&b).to_string(), value),
        };

        rest = updated_rest;
    }

    Err(format!(
        "Invalid Bencode Dict '{}'",
        String::from_utf8_lossy(input)
    ))
}

pub fn parse_bencode(input: &Vec<u8>) -> Result<(BencodeValue, Vec<u8>), String> {
    match input.get(0) {
        Some(char) => match char {
            b'i' => {
                let (int, rest) = parse_int(input)?;
                Ok((BencodeValue::Int(int), rest))
            }
            b'l' => {
                let (list, rest) = parse_list(input)?;
                Ok((BencodeValue::List(list), rest))
            }
            b'd' => {
                let (dict, rest) = parse_dict(input)?;
                Ok((BencodeValue::Dict(dict), rest))
            }
            _ => {
                let (string, rest) = parse_string(input)?;
                Ok((BencodeValue::String(string), rest))
            }
        },
        None => {
            return Err(format!(
                "Invalid Bencode Value '{}'",
                String::from_utf8_lossy(input)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_byte_vec(s: &str) -> Vec<u8> {
        s.bytes().collect::<Vec<u8>>()
    }
    #[test]
    fn test_parse_string() {
        assert_eq!(
            Ok((BencodeString::String("spam".to_string()), Vec::new())),
            parse_string(&to_byte_vec("4:spam"))
        );
        assert_eq!(
            Ok((
                BencodeString::String("spam".to_string()),
                to_byte_vec("remaining")
            )),
            parse_string(&to_byte_vec("4:spamremaining"))
        );
        assert_eq!(
            Ok((BencodeString::String("0123456789".to_string()), Vec::new())),
            parse_string(&to_byte_vec("10:0123456789"))
        );

        assert_eq!(
            Err("Invalid Bencode String 'invalid'".to_string()),
            parse_string(&to_byte_vec("invalid"))
        );
        assert_eq!(
            Err("Invalid Bencode String 'invalid:invalid'".to_string()),
            parse_string(&to_byte_vec("invalid:invalid"))
        );

        assert_eq!(
            Ok((BencodeString::String("a:b".to_string()), Vec::new())),
            parse_string(&to_byte_vec("3:a:b"))
        );
    }

    #[test]
    fn test_parse_int() {
        assert_eq!(Ok((3, Vec::new())), parse_int(&to_byte_vec("i3e")));
        assert_eq!(Ok((-3, Vec::new())), parse_int(&to_byte_vec("i-3e")));
        assert_eq!(Ok((0, Vec::new())), parse_int(&to_byte_vec("i0e")));
        assert_eq!(Ok((4096, Vec::new())), parse_int(&to_byte_vec("i4096e")));
        assert_eq!(
            Ok((0, to_byte_vec("4:spam"))),
            parse_int(&to_byte_vec("i0e4:spam"))
        );

        assert_eq!(
            Err("Invalid Bencode Integer 'i02e', cannot be prefixed with 0".to_string()),
            parse_int(&to_byte_vec("i02e"))
        );
        assert_eq!(
            Err("Invalid Bencode Integer 'i-0e'".to_string()),
            parse_int(&to_byte_vec("i-0e"))
        );
        assert_eq!(
            Err("Invalid Bencode Integer 'i-02e', cannot be prefixed with 0".to_string()),
            parse_int(&to_byte_vec("i-02e"))
        );
        assert_eq!(
            Err("Could not parse Bencode integer 'iinvalide'".to_string()),
            parse_int(&to_byte_vec("iinvalide"))
        );
    }

    #[test]
    fn test_parse_list() {
        assert_eq!(Ok((vec![], Vec::new())), parse_list(&to_byte_vec("le")));
        assert_eq!(
            Ok((
                vec![
                    BencodeValue::String(BencodeString::String("spam".to_string())),
                    BencodeValue::String(BencodeString::String("ham".to_string()))
                ],
                Vec::new()
            )),
            parse_list(&to_byte_vec("l4:spam3:hame"))
        );
        assert_eq!(
            Ok((
                vec![
                    BencodeValue::String(BencodeString::String("spam".to_string())),
                    BencodeValue::Int(123)
                ],
                Vec::new()
            )),
            parse_list(&to_byte_vec("l4:spami123ee"))
        );
        assert_eq!(
            Ok((
                vec![
                    BencodeValue::String(BencodeString::String("spam".to_string())),
                    BencodeValue::Int(123),
                    BencodeValue::List(vec![
                        BencodeValue::Int(1),
                        BencodeValue::Int(2),
                        BencodeValue::Int(3)
                    ])
                ],
                Vec::new()
            )),
            parse_list(&to_byte_vec("l4:spami123eli1ei2ei3eee"))
        );
        assert_eq!(
            Ok((
                vec![BencodeValue::Dict(BTreeMap::from([(
                    "test".to_string(),
                    BencodeValue::String(BencodeString::String("value".to_string()))
                )])),],
                Vec::new()
            )),
            parse_list(&to_byte_vec("ld4:test5:valueee"))
        );

        assert_eq!(
            Err("Invalid Bencode List 'invalid'".to_string()),
            parse_list(&to_byte_vec("invalid"))
        );
        assert_eq!(
            Err("Invalid Bencode List 'l'".to_string()),
            parse_list(&to_byte_vec("l"))
        );
    }

    #[test]
    fn test_parse_dict() {
        assert_eq!(
            Ok((BTreeMap::new(), Vec::new())),
            parse_dict(&to_byte_vec("de"))
        );
        assert_eq!(
            Ok((
                BTreeMap::from([
                    (
                        "spam".to_string(),
                        BencodeValue::String(BencodeString::String("egg".to_string()))
                    ),
                    ("cow".to_string(), BencodeValue::Int(3))
                ]),
                Vec::new()
            )),
            parse_dict(&to_byte_vec("d4:spam3:egg3:cowi3ee"))
        );
        assert_eq!(
            Ok((
                BTreeMap::from([
                    (
                        "spam".to_string(),
                        BencodeValue::String(BencodeString::String("egg".to_string()))
                    ),
                    ("cow".to_string(), BencodeValue::Int(3)),
                    (
                        "list".to_string(),
                        BencodeValue::List(vec![BencodeValue::Int(123)])
                    )
                ]),
                Vec::new()
            )),
            parse_dict(&to_byte_vec("d4:spam3:egg3:cowi3e4:listli123eee"))
        );

        assert_eq!(
            Err("Invalid Bencode Dict 'invalid'".to_string()),
            parse_dict(&to_byte_vec("invalid"))
        );
        assert_eq!(
            Err("Invalid Bencode Dict 'd'".to_string()),
            parse_dict(&to_byte_vec("d"))
        );
    }
}
