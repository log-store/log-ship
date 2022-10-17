use std::rc::Rc;

use std::time::{Duration};

use chrono::DateTime;

use lazy_static::lazy_static;
use regex::Regex;
use serde_json::{Map, Value};
use serde::{Serialize, Deserialize};

use thiserror::Error;

use crate::config::{TimestampFormat, ConfigFile};

use crate::log_value::LogValue;

// this is the time of the log entry, as described in the log itself
pub const DEFAULT_TIMESTAMP_FIELD: &str = "t";

// acceptable prefixes for field names
pub const FIELD_PREFIXES: &[char] = &['-', '+'];
lazy_static! {
    static ref FIELD_RE: Regex = {
        Regex::new(r#"^[\-+]?[[[:alpha:]]\d_][[[:alpha:]]\d\._\-]*$"#).expect("Invalid regular expression")
    };
}


#[derive(Error, Clone, Debug)]
pub enum LogEntryError {
    #[error("JSON Error: {0}")]
    Json(String),

    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

// pub type Field = Arc<str>;
pub type Field = Rc<str>;
// pub type Field = String;
// pub type FieldValues = SmallVec<[(Field, LogValue); 2]>;
pub type FieldValues = Vec<(Field, LogValue)>;

/// Representation of a single log entry.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LogEntry {
    field_values: FieldValues,
}

impl Eq for LogEntry { }

impl PartialEq for LogEntry {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }

        //TODO: Try ane make more efficient
        for (f, v) in self.field_values.iter() {
            let ov = other.get(f.clone());

            if ov.is_none() || ov.unwrap() != v {
                return false;
            }
        }

        true
    }
}

impl LogEntry {
    /// This is the workhorse method that converts from JSON to LogEntry
    pub fn from_json(value: Value, config_file: &ConfigFile) -> Result<Self, LogEntryError> {
        // ensure it's an object
        let json_map = if let Value::Object(map) = value {
            map
        } else {
            return Err(LogEntryError::InvalidFormat(format!("Log entry must be JSON object, found: {}", value.to_string())));
        };

        // if we don't have a timestamp, that's an error
        if !json_map.contains_key(config_file.timestamp_field.as_str()) {
            return Err(LogEntryError::InvalidFormat(format!("Missing timestamp field: {}", config_file.timestamp_field)));
        }

        // create a new entry, with the correct capacity
        let mut log_entry = LogEntry::with_capacity(json_map.len());

        // convert everything to a LogValue
        for (field, value) in json_map.into_iter() {
            if field == config_file.timestamp_field {
                let value = match &config_file.timestamp_format {
                    TimestampFormat::EPOCH => {
                        let ts = if let Some(ts) = value.as_u64() {
                            ts
                        } else if let Some(ts_str) = value.as_str() {
                            ts_str.parse::<u64>()
                                .map_err(|_e| LogEntryError::InvalidFormat(format!("Timestamp was not a positive integer: {}", value)))?
                        } else {
                            return Err(LogEntryError::InvalidFormat(format!("Timestamp was not a positive integer: {}", value)))
                        };

                        let num_digits = (ts as f64).log10() as usize;

                        // figure out if it's sec or ms: 11 digits or more, we consider it ms
                        if num_digits >= 14 {
                            LogValue::TimeStamp(Duration::from_micros(ts))
                        } else if num_digits >= 11 {
                            LogValue::TimeStamp(Duration::from_millis(ts))
                        } else {
                            LogValue::TimeStamp(Duration::from_secs(ts))
                        }
                    }
                    TimestampFormat::RFC2822 => {
                        let ts_str = value.as_str().ok_or(LogEntryError::InvalidFormat(format!("Timestamp was not a string: {}", value)))?;
                        let dt = DateTime::parse_from_rfc2822(ts_str).map_err(|e| LogEntryError::InvalidFormat(format!("Error parsing timestamp: {}", e)))?;
                        let dt_ms = dt.timestamp_millis();

                        if dt_ms < 0 {
                            return Err(LogEntryError::InvalidFormat("Timestamps cannot be before Jan 1 1970".to_string()));
                        }

                        LogValue::TimeStamp(Duration::from_millis(dt_ms as u64))
                    }
                    TimestampFormat::RFC3339 => {
                        let ts_str = value.as_str().ok_or(LogEntryError::InvalidFormat(format!("Timestamp was not a string: {}", value)))?;
                        let dt = DateTime::parse_from_rfc3339(ts_str).map_err(|e| LogEntryError::InvalidFormat(format!("Error parsing timestamp: {}", e)))?;
                        let dt_ms = dt.timestamp_millis();

                        if dt_ms < 0 {
                            return Err(LogEntryError::InvalidFormat("Timestamps cannot be before Jan 1 1970".to_string()));
                        }

                        LogValue::TimeStamp(Duration::from_millis(dt_ms as u64))
                    }
                    // TimestampFormat::FORMAT(format_string) => {
                    //     let ts_str = value.as_str().ok_or(LogEntryError::InvalidFormat(format!("Timestamp was not a string: {}", value)))?;
                    //     let dt = DateTime::parse_from_str(ts_str, format_string.as_str()).map_err(|e| LogEntryError::InvalidFormat(format!("Error parsing timestamp: {}", e)))?;
                    //     let dt_ms = dt.timestamp_millis();
                    //
                    //     if dt_ms < 0 {
                    //         return Err(LogEntryError::InvalidFormat("Timestamps cannot be before Jan 1 1970".to_string()));
                    //     }
                    //
                    //     LogValue::TimeStamp(Duration::from_millis(dt_ms as u64))
                    // }
                };

                log_entry.insert(field, value)
            } else if FIELD_RE.is_match(field.as_str()) {
                log_entry.insert(field, LogValue::from(value));
            } else {
                return Err(LogEntryError::InvalidFormat(format!("Field name contains invalid characters: {}", field)))
            }
        }

        Ok(log_entry)
    }

    pub fn from_default_json(value: Value) -> Result<Self, LogEntryError> {
        LogEntry::from_json(value, &ConfigFile::default())
    }

    #[inline(always)]
    pub fn with_capacity(capacity: usize) -> Self {
        LogEntry {
            field_values: FieldValues::with_capacity(capacity)
            // field_values: FieldValues::new()
        }
    }

    /// Constructs a new LogEntry from an Iterator that produces (String, LogValue) tuples
    pub fn from_iter<S: Into<Field>, I: Iterator<Item=(S, LogValue)>>(iter: I) -> Self {
        LogEntry {
            field_values: iter.map(|(f, v)| (f.into(), v)).collect::<FieldValues>()
        }
    }

    /// Gets a [LogValue] given a field name
    pub fn get<S: Into<Field>>(&self, field: S) -> Option<&LogValue> {
        let field: Field = field.into();

        self.field_values.iter().find(|(f, _v)| *f == field).map(|(_k, v)| v)
    }

    /// Sets a [LogValue] for a given field name, returning the old value if any
    pub fn set<S: Into<Field>>(&mut self, field: S, value: LogValue) -> Option<LogValue> {
        let field = field.into();
        let idx = self.field_values.iter().position(|(f, _v)| *f == field);

        if let Some(existing_idx) = idx {
            // set the value
            Some(std::mem::replace(&mut self.field_values[existing_idx].1, value))
        } else {
            // add it to the end
            self.field_values.push( (field, value) );
            None
        }
    }

    /// Like the [set] method, but doesn't do any checks... just inserts the values
    #[inline(always)]
    pub fn insert<S: Into<Field>>(&mut self, field: S, value: LogValue) {
        self.field_values.push((field.into(), value));
    }

    /// Constructs a new LogEntry with no prefixes on any of the field names
    pub fn trim_fields(&self) -> LogEntry {
        let mut ret = self.clone();

        for (field, _) in ret.field_values.iter_mut() {
            if FIELD_PREFIXES.contains(&field.chars().nth(0).unwrap()) {
                *field = field.trim_start_matches(FIELD_PREFIXES).into();
            }
        }

        ret
    }

    /// Returns the number of fields in the entry
    #[inline]
    pub fn len(&self) -> usize {
        self.field_values.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.field_values.is_empty()
    }

    // Get an iterator over the keys and values of the entry
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item=&(Field, LogValue)> {
        self.field_values.iter()
    }

    #[inline]
    pub fn into_iter(self) -> impl Iterator<Item=(Field, LogValue)> {
        self.field_values.into_iter()
    }

    pub fn trimmed_fields(&self) -> impl Iterator<Item=Field> + '_ {
        self.field_values
            .iter()
            .map(|(f, _v)| f.trim_start_matches(FIELD_PREFIXES).into())
    }
}

impl From<LogEntry> for Value {
    fn from(entry: LogEntry) -> Self {
        Value::from(&entry)
    }
}

impl From<&LogEntry> for Value {
    fn from(entry: &LogEntry) -> Self {
        let mut ret = Map::new();

        for (field, value) in entry.field_values.iter() {
            let field = (*field.clone()).to_owned();
            ret.insert(field, value.into());
        }

        Value::Object(ret)
    }
}

impl TryFrom<&str> for LogEntry {
    type Error = LogEntryError;

    fn try_from(json_str: &str) -> Result<Self, Self::Error> {
        let v: Value = serde_json::from_str(json_str)
            .map_err(|e| LogEntryError::Json(format!("Error parsing JSON: {:?}", e)))?;

        LogEntry::from_default_json(v)
    }
}

#[cfg(test)]
mod log_entry_tests {
    use std::time::Duration;
    use serde_json::Value;

    use crate::config::{ConfigFile, default_timestamp_field, TimestampFormat};
    use crate::log_entry::{Field, LogEntry};
    use crate::log_value::LogValue;
    use crate::test_utils::fake_log_entry_iterator;
    use crate::{init_test_logger};

    #[test]
    fn object_array() {
        init_test_logger();
        let json = r#"{ "t": 1647392005, "stream": "default", "hello": "world", "number": 7, "null": null, "bool": true, "obj": { "key": "value" }, "array": [3, "h", null] }"#;
        let le = LogEntry::try_from(json).expect("Error creating LogEntry");

        // println!("obj: {}", le.get("obj").expect("Did not find obj"));
        // println!("array: {}", le.get("array").expect("Did not find array"));

        assert_eq!(Duration::from_secs(1647392005), le.get(default_timestamp_field().as_str()).expect("Error getting t").as_timestamp().unwrap());
        assert_eq!(r#"{"key": "value"}"#, le.get("obj").expect("Error getting obj").as_string().unwrap());
        assert_eq!(r#"[3,"h",null]"#, le.get("array").expect("Error getting array").as_string().unwrap());
    }

    #[test]
    fn random_test() {
        let mut it = fake_log_entry_iterator(vec![
            ("t", LogValue::Integer(1.into())),
            ("stream", LogValue::String("".to_string())),
            ("host", LogValue::String("".to_string())),
            ("port", LogValue::Integer(7.into()))
        ]);

        for _ in 0..10 {
            let le = it.next().unwrap();

            assert!(le.get("t").is_some());
            assert!(le.get("stream").is_some());
            assert!(le.get("host").is_some());
            assert!(le.get("port").is_some());
        }
    }

    #[test]
    fn timestamp_fields_and_formats() {
        init_test_logger();
        let mut config = ConfigFile::default();

        config.timestamp_field = "time".to_string();
        config.timestamp_format = TimestampFormat::EPOCH;
        let json = r#"{ "time": 1647392005, "t": "hello", "hello": "world", "number": 7, "null": null, "bool": true, "obj": { "key": "value" }, "array": [3, "h", null] }"#;
        let value: Value = serde_json::from_str(json).expect("Error parsing JSON");
        let le = LogEntry::from_json(value, &config).expect("Error creating LogEntry");

        assert_eq!(Duration::from_secs(1647392005), le.get(Field::from("time")).expect("Error getting t").as_timestamp().unwrap());
        assert_eq!("hello", le.get("t").expect("Error getting t").as_string().unwrap());


        config.timestamp_field = "ts".to_string();
        config.timestamp_format = TimestampFormat::RFC2822;
        let json = r#"{ "ts": "Tue, 1 Jul 2003 10:52:37 -0400", "t": "hello", "hello": "world", "number": 7, "null": null, "bool": true, "obj": { "key": "value" }, "array": [3, "h", null] }"#;
        let value: Value = serde_json::from_str(json).expect("Error parsing JSON");
        let le = LogEntry::from_json(value, &config).expect("Error creating LogEntry");

        assert_eq!(Duration::from_secs(1057071157), le.get(Field::from("ts")).expect("Error getting t").as_timestamp().unwrap());
        assert_eq!("hello", le.get("t").expect("Error getting t").as_string().unwrap());


        config.timestamp_field = "time_stamp".to_string();
        config.timestamp_format = TimestampFormat::RFC3339;
        let json = r#"{ "time_stamp": "1996-12-19T16:39:57-05:00", "t": "hello", "hello": "world", "number": 7, "null": null, "bool": true, "obj": { "key": "value" }, "array": [3, "h", null] }"#;
        let value: Value = serde_json::from_str(json).expect("Error parsing JSON");
        let le = LogEntry::from_json(value, &config).expect("Error creating LogEntry");

        assert_eq!(Duration::from_secs(851031597), le.get(Field::from("time_stamp")).expect("Error getting t").as_timestamp().unwrap());
        assert_eq!("hello", le.get("t").expect("Error getting t").as_string().unwrap());
    }
}