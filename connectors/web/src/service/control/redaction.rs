use toml::{from_str, to_string_pretty, Table, Value as TomlValue};

pub(super) fn redact(text: String) -> (String, bool) {
    let Ok(mut doc) = from_str::<Table>(&text) else {
        return (text, false);
    };
    fn visit(value: &mut TomlValue, path: &mut Vec<String>) -> bool {
        if path.as_slice() == ["memory", "env"]
            && value.as_table().is_some_and(|table| {
                table
                    .iter()
                    .all(|(key, value)| env_name(key) && value.as_str().is_some_and(env_name))
            })
        {
            return false;
        }
        let mut changed = false;
        match value {
            TomlValue::Table(table) => {
                for (key, value) in table {
                    let lower = key.to_ascii_lowercase();
                    if matches!(
                        lower.as_str(),
                        "api_key"
                            | "password"
                            | "access_token"
                            | "refresh_token"
                            | "client_secret"
                            | "token"
                            | "authorization"
                            | "cookie"
                            | "bearer"
                    ) && value.is_str()
                    {
                        *value = TomlValue::String("[REDACTED]".into());
                        changed = true;
                    } else {
                        path.push(key.clone());
                        changed |= visit(value, path);
                        path.pop();
                    }
                }
            }
            TomlValue::Array(values) => {
                for value in values {
                    changed |= visit(value, path);
                }
            }
            _ => {}
        }
        changed
    }
    let mut value = TomlValue::Table(doc.clone());
    let changed = visit(&mut value, &mut Vec::new());
    if changed {
        doc = value.try_into().unwrap_or_default();
        (to_string_pretty(&doc).unwrap_or_default(), true)
    } else {
        (text, false)
    }
}

fn env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
}
