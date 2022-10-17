use std::fmt::{Display, Formatter};

// pub type JsonValue = simd_json::value::owned::Value;
pub type JsonValue = serde_json::Value;

#[derive(PartialEq, Clone, Debug)]
pub enum Event {
    None,
    Json(JsonValue),
    String(String)
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            Event::None => {
                write!(f, "None")
            }
            Event::Json(j) => {
                write!(f, "{}", j)
            }
            Event::String(s) => {
                f.write_str(s.as_str())
            }
        }
    }
}

impl From<&str> for Event {
    fn from(event_str: &str) -> Self {
        Event::String(event_str.to_string())
    }
}
