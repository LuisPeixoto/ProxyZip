//! Server-sent events (`text/event-stream`).

use serde_json::{Map, Value, json};

/// Parse a (possibly still growing) event stream into events. An incomplete trailing event is ignored.
pub fn parse(text: &str) -> Vec<Value> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut events = Vec::new();
    let complete = match text.rfind("\n\n") {
        Some(i) => &text[..i + 2],
        None => return events,
    };
    for block in complete.split("\n\n").filter(|b| !b.trim().is_empty()) {
        let mut event = Map::new();
        let mut data: Vec<&str> = Vec::new();
        for line in block.lines() {
            if line.starts_with(':') {
                continue;
            }
            let (field, value) =
                line.split_once(':').map_or((line, ""), |(f, v)| (f, v.strip_prefix(' ').unwrap_or(v)));
            match field {
                "data" => data.push(value),
                "id" | "event" => {
                    event.insert(field.into(), json!(value));
                }
                "retry" => {
                    event.insert("retry".into(), value.parse::<u64>().map_or_else(|_| json!(value), Value::from));
                }
                _ => {}
            }
        }
        if data.is_empty() && event.is_empty() {
            continue;
        }
        let joined = data.join("\n");
        if let Ok(parsed) = serde_json::from_str::<Value>(&joined)
            && !joined.is_empty()
            && matches!(parsed, Value::Object(_) | Value::Array(_))
        {
            event.insert("json".into(), parsed);
        }
        event.insert("data".into(), json!(joined));
        events.push(Value::Object(event));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_with_fields_and_multiline_data() {
        let stream =
            ": keep-alive\n\nid: 1\nevent: update\ndata: {\"n\":1}\n\ndata: line one\ndata: line two\nretry: 3000\n\n";
        let events = parse(stream);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["id"], "1");
        assert_eq!(events[0]["event"], "update");
        assert_eq!(events[0]["json"]["n"], 1);
        assert_eq!(events[1]["data"], "line one\nline two");
        assert_eq!(events[1]["retry"], 3000);
        assert!(events[1].get("json").is_none());
    }

    #[test]
    fn ignores_the_unfinished_tail_and_handles_crlf() {
        let events = parse("data: a\r\n\r\ndata: partial");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["data"], "a");
        assert!(parse("data: nothing complete yet").is_empty());
    }
}
