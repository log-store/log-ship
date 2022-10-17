use std::cmp::Ordering;
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Number;
use serde_json::Value as JsonValue;
use thiserror::Error;

use crate::round_duration;

#[derive(Error, Debug)]
pub enum LogValueError {
    #[error("LogValue Serialization Error: {0}")]
    Serialization(String),

    #[error("LogValue Deserialization Error: {0}")]
    Deserialization(String),
}

#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum ValueType {
    Null = 0,
    True = 1,
    False = 2,
    Integer = 4,
    Float = 5,
    String = 6,
    TimeStamp = 7,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum LogValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    TimeStamp(Duration)
    // arrays and nested objects are not supported
}

impl LogValue {
    /// Converts a ValueType and byte slice to a LogValue
    /// Not Null, True, and False do not even read the buff
    pub fn from_type_bytes(value_type: ValueType, buff: &[u8]) -> Result<LogValue, LogValueError> {
        match value_type {
            ValueType::Null => { Ok(LogValue::Null) }
            ValueType::True => { Ok(LogValue::Bool(true)) }
            ValueType::False => { Ok(LogValue::Bool(false)) }
            ValueType::Integer => {
                let num = i64::from_ne_bytes(buff.try_into().map_err(|e| LogValueError::Deserialization(format!("{:?}",e)))?);

                Ok(LogValue::Integer(num))
            }
            ValueType::Float => {
                let num = f64::from_ne_bytes(buff.try_into().map_err(|e| LogValueError::Deserialization(format!("{:?}",e)))?);

                Ok(LogValue::Float(num))
            }
            ValueType::String => unsafe {
                let s = String::from_utf8_unchecked(buff.to_vec());

                Ok(LogValue::String(s))
            }
            ValueType::TimeStamp => {
                let ms = u64::from_ne_bytes(buff.try_into().map_err(|e| LogValueError::Deserialization(format!("{:?}", e)))?);

                Ok(LogValue::TimeStamp(Duration::from_millis(ms)))
            }
        }
    }

    /// Converts a LogValue into a byte slice, which can then be read back via from_slice
    pub fn to_type_bytes(&self) -> (ValueType, Vec<u8>) {
        match &self {
            LogValue::Null => { (ValueType::Null, Vec::new()) }
            LogValue::Bool(b) => {
                if *b {
                    (ValueType::True, Vec::new())
                } else {
                    (ValueType::False, Vec::new())
                }
            }
            LogValue::Integer(i) => { (ValueType::Integer, i.to_ne_bytes().to_vec()) }
            LogValue::Float(f) => { (ValueType::Float, f.to_ne_bytes().to_vec()) }
            LogValue::String(s) => { (ValueType::String, s.as_bytes().to_vec()) }
            LogValue::TimeStamp(d) => { (ValueType::TimeStamp, (d.as_millis() as u64).to_ne_bytes().to_vec() ) }
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            LogValue::Bool(b) => Some(*b),
            _ => None
        }
    }

    /// Returns the value as an i64 if it is one, None otherwise
    pub fn as_int(&self) -> Option<i64> {
        if let LogValue::Integer(n) = self {
            Some(*n)
        } else {
            None
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let LogValue::Integer(n) = self {
            Some(*n as f64)
        } else if let LogValue::Float(f) = self {
            Some(*f)
        } else {
            None
        }
    }

    pub fn as_string(&self) -> Option<String> {
        match self {
            LogValue::String(s) => Some(s.clone()),
            _ => None
        }
    }

    pub fn as_timestamp(&self) -> Option<Duration> {
        match self {
            LogValue::TimeStamp(d) => Some(*d),
            _ => None
        }
    }

    /// Convert the value into the bytes used in the index
    pub fn as_index_bytes(&self) -> Vec<u8> {
        match self {
            LogValue::Null => { vec![0x00] }
            LogValue::Bool(b) => { if *b { vec![0x01] } else { vec![0x00] } }
            LogValue::Integer(n) => { n.to_be_bytes().to_vec() }
            LogValue::Float(f) => { f.to_be_bytes().to_vec() }
            LogValue::String(s) => { s.clone().into_bytes() }
            LogValue::TimeStamp(d) => {
                // index_bytes are _always_ rounded down
                round_duration(*d).as_secs()
                    .to_be_bytes()
                    .to_vec()
            }
        }
    }
}

impl Default for LogValue {
    fn default() -> Self {
        LogValue::Null
    }
}

impl Display for LogValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            LogValue::Null => write!(f, "Null"),
            LogValue::Bool(v) => write!(f, "{}", v),
            LogValue::Integer(ref v) => write!(f, "{}", v),
            LogValue::Float(ref v) => write!(f, "{}", v),
            LogValue::String(ref v) => write!(f, "'{}'", v),
            LogValue::TimeStamp(ref v) => write!(f, "{}", v.as_millis())
        }
    }
}

impl From<JsonValue> for LogValue {
    fn from(v: JsonValue) -> LogValue {
        LogValue::from(&v)
    }
}

impl From<&JsonValue> for LogValue {
    fn from(v: &JsonValue) -> Self {
        match v {
            JsonValue::Null => LogValue::Null,
            JsonValue::Bool(b) => LogValue::Bool(*b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    LogValue::Integer(i)
                } else {
                    // this will always work
                    LogValue::Float(n.as_f64().unwrap())
                }
            },
            JsonValue::String(s) => LogValue::String(s.clone()),
            // TODO: Attempt to parse a date as a TimeStamp
            JsonValue::Array(a) => {
                let value_str = a.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                LogValue::String(format!("[{}]", value_str))
            },
            JsonValue::Object(o) => {
                let value_str = o.iter().map(|(k,v)| format!(r#""{}": {}"#, k, v)).collect::<Vec<_>>().join(",");
                LogValue::String(format!("{{{}}}", value_str))
            }
        }
    }
}

impl From<LogValue> for JsonValue {
    fn from(value: LogValue) -> Self {
        JsonValue::from(&value)
    }
}

impl From<&LogValue> for JsonValue {
    fn from(value: &LogValue) -> Self {
        match value {
            LogValue::Null => JsonValue::Null,
            LogValue::Bool(b) => JsonValue::Bool(*b),
            LogValue::Integer(n) => JsonValue::from(*n),
            LogValue::Float(n) => JsonValue::from(*n),
            LogValue::String(s) => JsonValue::String(s.clone()),
            LogValue::TimeStamp(d) => {
                // JavaScript likes ms for epoch
                let ms = d.as_millis();

                JsonValue::Number(Number::from(ms as u64))
            }
        }
    }
}

impl From<&str> for LogValue {
    fn from(s: &str) -> Self {
        match s {
            "null" => LogValue::Null,
            "true" | "True" => LogValue::Bool(true),
            "false" | "False" => LogValue::Bool(false),
            _ => {
                // try to parse as a number first, finally just a string
                // TODO: Handle timestamps
                if let Ok(n) = s.parse::<u64>() {
                    LogValue::from(n)
                } else if let Ok(n) = s.parse::<i64>() {
                    LogValue::from(n)
                } else if let Ok(n) = s.parse::<f64>() {
                    LogValue::from(n)
                } else {
                    LogValue::String(s.to_string())
                }
            }
        }
    }
}

impl From<u64> for LogValue {
    fn from(n: u64) -> Self {
        LogValue::Integer(n as i64)
    }
}

impl From<i64> for LogValue {
    fn from(n: i64) -> Self {
        LogValue::Integer(n)
    }
}

impl From<f64> for LogValue {
    fn from(n: f64) -> Self {
        LogValue::Float(n)
    }
}

impl From<Duration> for LogValue {
    fn from(d: Duration) -> Self {
        LogValue::TimeStamp(d)
    }
}

impl Hash for LogValue {
    fn hash<H>(&self, state: &mut H) where H: Hasher {
        match self {
            &LogValue::Null => 0.hash(state),
            &LogValue::Bool(b) => b.hash(state),
            &LogValue::Integer(ref n) => state.write(&n.to_ne_bytes()),
            &LogValue::Float(ref n) => state.write(&n.to_ne_bytes()),
            &LogValue::String(ref s) => s.hash(state),
            &LogValue::TimeStamp(ref d) => d.hash(state)
        }
    }
}

impl PartialOrd for LogValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (&LogValue::Null, &LogValue::Null) => Some(Ordering::Equal),
            (&LogValue::Bool(b1), &LogValue::Bool(b2)) => Some(b1.cmp(&b2)),
            (&LogValue::Integer(ref n1), &LogValue::Integer(ref n2)) => Some(n1.cmp(&n2)),
            (&LogValue::Float(ref n1), &LogValue::Float(ref n2)) => n1.partial_cmp(&n2),
            (&LogValue::String(ref s1), &LogValue::String(ref s2)) => Some(s1.cmp(&s2)),
            (&LogValue::TimeStamp(ref d1), &LogValue::TimeStamp(ref d2)) => Some(d1.cmp(&d2)),
            _ => None
        }
    }
}

impl Eq for LogValue { }

impl PartialEq for LogValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (&LogValue::Null, &LogValue::Null) => true,
            (&LogValue::Bool(b1), &LogValue::Bool(b2)) => b1 == b2,
            (&LogValue::Integer(ref n1), &LogValue::Integer(ref n2)) => n1 == n2,
            (&LogValue::Float(ref n1), &LogValue::Float(ref n2)) => n1 == n2,
            (&LogValue::String(ref s1), &LogValue::String(ref s2)) => s1 == s2,
            (&LogValue::TimeStamp(ref d1), &LogValue::TimeStamp(ref d2)) => d1 == d2,
            _ => false
        }
    }
}


#[cfg(test)]
mod log_value_tests {
    use std::time::Duration;

    use crate::LogValue;

    #[test]
    fn as_bytes_order() {
        let a_bytes = LogValue::Integer(0x56789i64).as_index_bytes();
        let b_bytes = LogValue::from(0x12345u64).as_index_bytes();

        println!("{:?} > {:?}", a_bytes, b_bytes);
        assert!(a_bytes > b_bytes);

        let a_bytes = LogValue::from(299445174u64).as_index_bytes();
        let b_bytes = LogValue::from(660210456u64).as_index_bytes();

        println!("{:?} < {:?}", a_bytes, b_bytes);
        assert!(a_bytes < b_bytes);
    }

    #[test]
    fn to_type_bytes() {
        let lv = LogValue::Null;
        let (t, b) = lv.to_type_bytes();
        let v = LogValue::from_type_bytes(t, b.as_slice()).unwrap();
        assert_eq!(lv, v);

        let lv = LogValue::Bool(true);
        let (t, b) = lv.to_type_bytes();
        let v = LogValue::from_type_bytes(t, b.as_slice()).unwrap();
        assert_eq!(lv, v);

        let lv = LogValue::Bool(false);
        let (t, b) = lv.to_type_bytes();
        let v = LogValue::from_type_bytes(t, b.as_slice()).unwrap();
        assert_eq!(lv, v);

        let lv = LogValue::Integer(-34567);
        let (t, b) = lv.to_type_bytes();
        let v = LogValue::from_type_bytes(t, b.as_slice()).unwrap();
        assert_eq!(lv, v);

        let lv = LogValue::Float(-1234.5678);
        let (t, b) = lv.to_type_bytes();
        let v = LogValue::from_type_bytes(t, b.as_slice()).unwrap();
        assert_eq!(lv, v);

        let lv = LogValue::String("hello world".to_string());
        let (t, b) = lv.to_type_bytes();
        let v = LogValue::from_type_bytes(t, b.as_slice()).unwrap();
        assert_eq!(lv, v);

        let lv = LogValue::TimeStamp(Duration::new(234567, 0));
        let (t, b) = lv.to_type_bytes();
        let v = LogValue::from_type_bytes(t, b.as_slice()).unwrap();
        assert_eq!(lv, v);
    }
}