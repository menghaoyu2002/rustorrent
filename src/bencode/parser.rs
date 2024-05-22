use std::collections::HashMap;

use super::BencodeValue;

fn parse_string(input: &str) -> Result<(String, &str), String> {
    let (str_len, rest) = match input.split_once(":") {
        Some((len_str, rest)) => (
            len_str
                .parse::<usize>()
                .map_err(|_| format!("Invalid string length '{}'", len_str))?,
            rest,
        ),
        None => return Err(format!("Invalid Bencode String '{}'", input)),
    };

    Ok((rest[..str_len].to_string(), &rest[str_len..]))
}

fn parse_int(input: &str) -> Result<(i64, &str), String> {
    if !input.starts_with("i") {
        return Err(format!("Invalid Bencode Integer '{}'", input));
    }

    let rest = &input[1..];

    match rest.split_once("e") {
        Some((str_int, rest)) => {
            if str_int != "0" && str_int.starts_with("0") {
                return Err(format!(
                    "Invalid Bencode Integer '{}', cannot be prefix with 0",
                    str_int
                ));
            }

            if str_int.starts_with("-0") {
                return Err(format!("Invalid Bencode Integer '{}'", str_int));
            }

            Ok((
                str_int
                    .parse::<i64>()
                    .map_err(|_| format!("Could not parse Bencode integer '{}'", str_int))?,
                rest,
            ))
        }
        None => Err(format!("Invalid Benode Integer '{}'", input)),
    }
}

fn parse_list(input: &str) -> Result<(Vec<BencodeValue>, &str), String> {
    if !input.starts_with("l") {
        return Err(format!("Invalid Bencode List '{}'", input));
    }
    let mut rest = &input[1..];

    let mut list = Vec::new();
    while !rest.is_empty() && !rest.starts_with("e") {
        let (value, new_rest) = parse_bencode(rest)?;
        list.push(value);
        rest = new_rest;
    }

    if rest.is_empty() {
        return Err(format!("Invalid Bencode List '{}'", input));
    }

    Ok((list, &rest[1..]))
}

fn parse_dict(input: &str) -> Result<(HashMap<String, BencodeValue>, &str), String> {
    if !input.starts_with("d") {
        return Err(format!("Invalid Bencode Dict '{}'", input));
    }

    let mut rest = &input[1..];

    let mut dict = HashMap::new();
    while !rest.is_empty() && !rest.starts_with("e") {
        let (key, new_rest) = parse_string(rest)?;
        rest = new_rest;

        let (value, new_rest) = parse_bencode(rest)?;
        rest = new_rest;

        dict.insert(key, value);
    }

    if rest.is_empty() {
        return Err(format!("Invalid Bencode Dict '{}'", input));
    }

    Ok((dict, &rest[1..]))
}

pub fn parse_bencode(input: &str) -> Result<(BencodeValue, &str), String> {
    match input.get(..1) {
        Some(char) => match char {
            "i" => {
                let (int, rest) = parse_int(input)?;
                Ok((BencodeValue::Int(int), rest))
            }
            "l" => {
                let (list, rest) = parse_list(input)?;
                Ok((BencodeValue::List(list), rest))
            }
            "d" => {
                let (dict, rest) = parse_dict(input)?;
                Ok((BencodeValue::Dict(dict), rest))
            }
            _ => {
                let (string, rest) = parse_string(input)?;
                Ok((BencodeValue::String(string), rest))
            }
        },
        None => return Err(format!("Invalid Bencode Value '{}'", input)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_string() {
        assert_eq!(Ok(("spam".to_string(), "")), parse_string("4:spam"));
        assert_eq!(
            Ok(("spam".to_string(), "remaining")),
            parse_string("4:spamremaining")
        );
        assert_eq!(
            Ok(("0123456789".to_string(), "")),
            parse_string("10:0123456789")
        );

        assert_eq!(
            Err("Invalid Bencode String 'invalid'".to_string()),
            parse_string("invalid")
        );
        assert_eq!(
            Err("Invalid string length 'invalid'".to_string()),
            parse_string("invalid:invalid")
        );

        assert_eq!(Ok(("a:b".to_string(), "")), parse_string("3:a:b"));
    }

    #[test]
    fn test_parse_int() {
        assert_eq!(Ok((3, "")), parse_int("i3e"));
        assert_eq!(Ok((-3, "")), parse_int("i-3e"));
        assert_eq!(Ok((0, "")), parse_int("i0e"));
        assert_eq!(Ok((4096, "")), parse_int("i4096e"));
        assert_eq!(Ok((0, "4:spam")), parse_int("i0e4:spam"));

        assert_eq!(
            Err("Invalid Bencode Integer '02', cannot be prefix with 0".to_string()),
            parse_int("i02e")
        );
        assert_eq!(
            Err("Invalid Bencode Integer '-0'".to_string()),
            parse_int("i-0e")
        );
        assert_eq!(
            Err("Invalid Bencode Integer '-02'".to_string()),
            parse_int("i-02e")
        );
        assert_eq!(
            Err("Could not parse Bencode integer 'invalid'".to_string()),
            parse_int("iinvalide")
        );
    }

    #[test]
    fn test_parse_list() {
        assert_eq!(Ok((vec![], "")), parse_list("le"));
        assert_eq!(
            Ok((
                vec![
                    BencodeValue::String("spam".to_string()),
                    BencodeValue::String("ham".to_string())
                ],
                ""
            )),
            parse_list("l4:spam3:hame")
        );
        assert_eq!(
            Ok((
                vec![
                    BencodeValue::String("spam".to_string()),
                    BencodeValue::Int(123)
                ],
                ""
            )),
            parse_list("l4:spami123ee")
        );
        assert_eq!(
            Ok((
                vec![
                    BencodeValue::String("spam".to_string()),
                    BencodeValue::Int(123),
                    BencodeValue::List(vec![
                        BencodeValue::Int(1),
                        BencodeValue::Int(2),
                        BencodeValue::Int(3)
                    ])
                ],
                ""
            )),
            parse_list("l4:spami123eli1ei2ei3eee")
        );
        assert_eq!(
            Ok((
                vec![BencodeValue::Dict(HashMap::from([(
                    "test".to_string(),
                    BencodeValue::String("value".to_string())
                )])),],
                ""
            )),
            parse_list("ld4:test5:valueee")
        );

        assert_eq!(
            Err("Invalid Bencode List 'invalid'".to_string()),
            parse_list("invalid")
        );
        assert_eq!(Err("Invalid Bencode List 'l'".to_string()), parse_list("l"));
    }

    #[test]
    fn test_parse_dict() {
        assert_eq!(Ok((HashMap::new(), "")), parse_dict("de"));
        assert_eq!(
            Ok((
                HashMap::from([
                    ("spam".to_string(), BencodeValue::String("egg".to_string())),
                    ("cow".to_string(), BencodeValue::Int(3))
                ]),
                ""
            )),
            parse_dict("d4:spam3:egg3:cowi3ee")
        );
        assert_eq!(
            Ok((
                HashMap::from([
                    ("spam".to_string(), BencodeValue::String("egg".to_string())),
                    ("cow".to_string(), BencodeValue::Int(3)),
                    (
                        "list".to_string(),
                        BencodeValue::List(vec![BencodeValue::Int(123)])
                    )
                ]),
                ""
            )),
            parse_dict("d4:spam3:egg3:cowi3e4:listli123eee")
        );

        assert_eq!(
            Err("Invalid Bencode Dict 'invalid'".to_string()),
            parse_dict("invalid")
        );
        assert_eq!(Err("Invalid Bencode Dict 'd'".to_string()), parse_dict("d"));
    }
}
