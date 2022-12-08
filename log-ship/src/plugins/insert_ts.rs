use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use serde_json::json;
use chrono::Utc;
use stream_cancel::{StreamExt, Tripwire};
use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::BroadcastStream;
use toml::Value;


use crate::common::logging::{error, warn};
use crate::event::{Event, JsonValue};
use crate::{Args, Plugin, send_event, connect_receiver, create_event_stream, get_receiver, recv_event, create_sender_semaphore};
use crate::plugin::{PluginType, ChannelType};


pub struct InsertTimestampTransform {
    tripwire: Tripwire,
    receiver: Option<Receiver<ChannelType>>,
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    field: String,
    ts_type: String,
    overwrite: bool,
}

#[async_trait]
impl Plugin for InsertTimestampTransform {
    fn name() -> &'static str {
        "insert_ts"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> {
        let field = args.get("field").unwrap_or(&Value::String("t".to_string())).to_owned();
        let field = field.as_str().ok_or(anyhow!("The 'field' arg for {} does not appear to be a string", Self::name()))?;

        let ts_type = args.get("ts_type").unwrap_or(&Value::String("epoch".to_string())).to_owned();
        let ts_type = ts_type.as_str().ok_or(anyhow!("The 'ts_type' arg for {} does not appear to be a string", Self::name()))?;

        match ts_type {
            "epoch" | "EPOCH" | "rfc2822" | "RFC2822" | "rfc3339" | "RFC3339" => (),
            _ => bail!("Timestamp type is unknown: {}", ts_type),
        }

        let overwrite = args.get("overwrite").unwrap_or(&Value::Boolean(false));
        let overwrite = overwrite.as_bool().ok_or_else(|| anyhow!("The 'overwrite' are for {} does not appear to be a bool", Self::name()))?;

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        Ok(Box::new(InsertTimestampTransform {
            tripwire,
            receiver: None, // set in connect_receiver
            sender,
            semaphore,
            field: field.to_string(),
            ts_type: ts_type.to_string(),
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
                            let cur_time = Utc::now();

                            let ts = match self.ts_type.as_str() {
                                "epoch" | "EPOCH" => json!(cur_time.timestamp_millis()),
                                "rfc2822" | "RFC2822" => serde_json::Value::String(cur_time.to_rfc2822()),
                                "rfc3339" | "RFC3339" => serde_json::Value::String(cur_time.to_rfc3339()),
                                _ => { panic!("Unknown timestamp format: {}", self.ts_type) }
                            };

                            if self.overwrite {
                                obj.insert(self.field.clone(), ts);
                            } else if !obj.contains_key(&self.field) {
                                obj.insert(self.field.clone(), ts);
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
