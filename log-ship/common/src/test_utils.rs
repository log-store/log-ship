use std::fmt::Debug;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use fake::{Fake, Faker};
use serde_json::{Map, Value};

#[allow(unused_imports)]
use crate::logging::{debug, error, info, warn};

use crate::log_entry::LogEntry;
use crate::log_value::LogValue;


pub struct FakeLogEntryIterator {
    template: Vec<(String, LogValue)>
}

impl Iterator for FakeLogEntryIterator {
    type Item = LogEntry;

    fn next(&mut self) -> Option<Self::Item> {
        let mut map = Map::new();

        for (k, v) in self.template.iter() {
            match v {
                LogValue::Null => map.insert(k.clone(), Value::Null),
                LogValue::Bool(_) => map.insert(k.clone(), Value::Bool(Faker.fake())),
                LogValue::Integer(_) => map.insert(k.clone(), Value::Number(Faker.fake::<u16>().into())),
                LogValue::Float(_) => map.insert(k.clone(), Value::from(Faker.fake::<f32>())),
                LogValue::String(_) => map.insert(k.clone(), Value::String(Faker.fake())),
                LogValue::TimeStamp(_) => map.insert(k.clone(), Value::Number(Faker.fake::<u64>().into()))
            };
        }

        Some(LogEntry::from_default_json(Value::Object(map)).unwrap())
    }
}

/// Returns a Iterator that generates LogEntries with the given keys
pub fn fake_log_entry_iterator(template :Vec::<(&str, LogValue)>) -> FakeLogEntryIterator {
    FakeLogEntryIterator {
        template: template.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<Vec<_>>()
    }
}


pub struct LogEntryFileIterator {
    file: BufReader<File>
}

impl Iterator for LogEntryFileIterator {
    type Item = LogEntry;

    fn next(&mut self) -> Option<Self::Item> {
        let mut json = String::new();

        if let Err(_e) = self.file.read_line(&mut json) {
            return None;
        }

        match LogEntry::try_from(json.as_str()) {
            Err(e) => {
                error!("Error decoding JSON: {:?}", e);
                None
            },
            Ok(le) => Some(le)
        }
    }
}

#[allow(unused)]
pub fn file_log_entry_iterator<P: Into<PathBuf> + Debug>(file_path: P) -> LogEntryFileIterator {
    let file_path = file_path.into();
    let file = BufReader::new(File::open(&file_path).expect(format!("Error opening file: {:?}", file_path).as_str()));

    LogEntryFileIterator {
        file
    }
}

pub struct JsonFileIterator {
    file: BufReader<File>
}

impl Iterator for JsonFileIterator {
    type Item = Value;

    fn next(&mut self) -> Option<Self::Item> {
        let mut json = String::new();

        if let Err(_e) = self.file.read_line(&mut json) {
            return None;
        }

        match serde_json::from_str(json.as_str()) {
            Err(e) => {
                error!("Error decoding JSON: {:?}", e);
                None
            },
            Ok(v) => Some(v)
        }
    }
}

#[allow(unused)]
pub fn file_json_iterator<P: Into<PathBuf> + Debug>(file_path: P) -> JsonFileIterator {
    let file_path = file_path.into();
    let file = BufReader::new(File::open(&file_path).expect(format!("Error opening file: {:?}", file_path).as_str()));

    JsonFileIterator {
        file
    }
}
