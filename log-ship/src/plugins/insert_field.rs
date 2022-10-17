use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use stream_cancel::{StreamExt, Tripwire};
use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::BroadcastStream;
use toml::{Value as TomlValue, Value};

use crate::common::logging::{error, warn};
use crate::event::{Event, JsonValue};
use crate::{Args, Plugin, send_event, connect_receiver, create_event_stream, get_receiver, recv_event, create_sender_semaphore};
use crate::plugin::{PluginType, ChannelType};


pub struct InsertFieldTransform {
    tripwire: Tripwire,
    receiver: Option<Receiver<ChannelType>>,
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    field: String,
    value: JsonValue,
    overwrite: bool,
}

fn toml2jsonvalue(value: &TomlValue) -> Result<JsonValue> {
    match value {
        Value::String(s) => { Ok(JsonValue::from(s.clone())) }
        Value::Integer(i) => { Ok(JsonValue::from(*i)) }
        Value::Float(f) => { Ok(JsonValue::from(*f)) }
        Value::Boolean(b) => { Ok(JsonValue::from(*b)) }
        Value::Array(_) => { Err(anyhow!("Array not a valid JSON value")) }
        Value::Datetime(_) => { Err(anyhow!("DateTime not a valid JSON value")) }
        Value::Table(_) => { Err(anyhow!("Table not a valid JSON value")) }
    }
}

#[async_trait]
impl Plugin for InsertFieldTransform {
    fn name() -> &'static str {
        "insert_field"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> {
        let field = args.get("field").ok_or(anyhow!("Could not find 'field' arg for {}", Self::name()))?;
        let field = field.as_str().ok_or(anyhow!("The 'field' arg for {} does not appear to be a string", Self::name()))?;

        let value = args.get("value").ok_or(anyhow!("Could not find 'value' arg for {}", Self::name()))?;
        let value = toml2jsonvalue(value)?;

        let overwrite = args.get("overwrite").unwrap_or(&Value::Boolean(false));
        let overwrite = overwrite.as_bool().ok_or_else(|| anyhow!("The 'overwrite' are for {} does not appear to be a bool", Self::name()))?;

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args);

        Ok(Box::new(InsertFieldTransform {
            tripwire,
            receiver: None, // set in connect_receiver
            sender,
            semaphore,
            field: field.to_string(),
            value,
            overwrite
        }))
    }

    async fn run(&mut self) {
        let mut event_stream = create_event_stream!(self);

        // grab an event and pass it along
        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);

            let event = match event {
                Event::None => {
                    continue // nothing to do here
                }
                Event::Json(json) => {
                    match json {
                        JsonValue::Object(mut obj) => {
                            if self.overwrite {
                                obj.insert(self.field.clone(), self.value.clone());
                            } else if !obj.contains_key(&self.field) {
                                obj.insert(self.field.clone(), self.value.clone());
                            }

                            Event::Json(JsonValue::from(obj))
                        }
                        _ => {
                            error!("Invalid JSON value received");
                            return
                        }
                    }
                }
                Event::String(_) => {
                    warn!("Found non-JSON log, skipping {}", Self::name());
                    continue
                }
            };

            // just sent along the event
            send_event!(self, event, callback);
        }
    }

    // boilerplate methods
    get_receiver!{}
    connect_receiver!{}
}
