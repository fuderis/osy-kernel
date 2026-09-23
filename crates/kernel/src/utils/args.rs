use serde_json::{Map, Value as JsonValue};

pub fn parse_cli_kv(input: &str) -> JsonValue {
    let mut map = Map::new();
    let tokens = tokenize_cli(input);

    for (idx, token) in tokens.into_iter().enumerate() {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        if let Some((key, val)) = token.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                map.insert(key.to_string(), parse_primitive_value(val.trim()));
            }
        } else if idx == 0 {
            // replace the first argument without '=' -> with action = "<value>"
            map.insert("action".to_string(), parse_primitive_value(token));
        } else {
            // all other arguments WITHOUT '=' are strictly ignored/skipped.
            // (or you can abort the parsing/return an error if necessary)
            continue;
        }
    }

    JsonValue::Object(map)
}

/// Tokenizer that takes into account quotation marks ("..." and '...') and spaces around '='.
pub fn tokenize_cli(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // quotation mark processing
            '"' | '\'' => {
                if let Some(q) = in_quotes {
                    if q == c {
                        in_quotes = None; // Closed quotation mark
                    } else {
                        current.push(c); // Another type of inner quotation mark
                    }
                } else {
                    in_quotes = Some(c); // Opened quotation mark
                }
            }
            // skipping spaces AROUND '='
            ' ' if in_quotes.is_none() => {
                // ignoring the space before the '='
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                if chars.peek() == Some(&'=') {
                    continue;
                }

                // ignore space after the '=' (if current ends with '=')
                if current.ends_with('=') {
                    continue;
                }

                // in all other cases, a space is a token separator.
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

pub fn parse_primitive_value(val: &str) -> JsonValue {
    match val {
        "true" => JsonValue::Bool(true),
        "false" => JsonValue::Bool(false),
        "null" => JsonValue::Null,
        _ => {
            if let Ok(num) = val.parse::<i64>() {
                JsonValue::Number(num.into())
            } else if let Ok(num) = val.parse::<f64>() {
                serde_json::Number::from_f64(num)
                    .map(JsonValue::Number)
                    .unwrap_or_else(|| JsonValue::String(val.to_string()))
            } else {
                JsonValue::String(val.to_string())
            }
        }
    }
}
