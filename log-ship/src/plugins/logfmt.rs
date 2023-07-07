use std::sync::Arc;
use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::{Map, Value};

use stream_cancel::Tripwire;
use tokio::sync::{broadcast, Mutex, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::BroadcastStream;
use stream_cancel::StreamExt;
use crate::plugin::{Args, ChannelType, Plugin, PluginType};
use crate::common::logging::{debug, error, warn};
use crate::{connect_receiver, create_event_stream, create_sender_semaphore, get_receiver, recv_event, send_event};
use crate::event::Event;

pub struct LogFmtParser {
    field: String,
    overwrite: bool,
    tripwire: Tripwire,
    receiver: Option<Receiver<ChannelType>>,
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
}

#[async_trait]
impl Plugin for LogFmtParser {
    fn name() -> &'static str where Self: Sized {
        "logfmt"
    }

    async fn new(args: Args, tripwire: Tripwire) -> anyhow::Result<Box<PluginType>> where Self: Sized {
        debug!("LogFmtParser args: {:#?}", args);

        let field = args.get("field").ok_or_else(|| anyhow!("The 'field' arg for {} is required", Self::name()))?.to_owned();
        let field = field.as_str().ok_or_else(|| anyhow!("The 'field' arg for {} does not appear to be a string", Self::name()))?.to_string();

        let overwrite = args.get("overwrite").unwrap_or(&toml::Value::from(false)).to_owned();
        let overwrite = overwrite.as_bool().ok_or_else(|| anyhow!("The optional field 'overwrite' for {} does not appear to be a boolean", Self::name()))?;

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        Ok(Box::new(LogFmtParser {
            field,
            overwrite,
            tripwire: tripwire.clone(),
            receiver: None,
            sender,
            semaphore,
        }))
    }

    async fn run(&mut self) {
        debug!("LogFmtParser running...");

        let mut event_stream = create_event_stream!(self);

        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);

            debug!("GOT EVENT: {:?}", event);

            match event {
                Event::None => continue,
                Event::Json(mut json) => {
                    let json = match json.as_object_mut() {
                        None => {
                            warn!("JSON not an object!");
                            continue;
                        }
                        Some(j) => j
                    };

                    if let Some(field) = json.remove(self.field.as_str()) {
                        if let Some(field_str) = field.as_str() {
                            let pairs = logfmt::parse(field_str);

                            // add all these to our existing JSON
                            for pair in pairs {
                                debug!("PAIR: {:?}", pair);

                                let value = Value::from(pair.val);

                                // check to see if we overwrite, or derive a new key
                                let key = if json.contains_key(pair.key.as_str()) && !self.overwrite {
                                    format!("{}.{}", self.field, pair.key)
                                } else {
                                    pair.key
                                };

                                json.insert(key, value);
                            }
                        }
                    }

                    // send the event
                    let event = Event::Json(Value::Object(json.clone()));
                    send_event!(self, event, callback);
                }
                Event::String(_) => {
                    warn!("Received text; expecting JSON");
                    continue
                }
            }

        }
    }

    // boilerplate methods
    get_receiver!{}
    connect_receiver!{}
}


// #[cfg(test)]
// mod logfmt_tests {
//     use std::fs;
//     use stream_cancel::Tripwire;
//     use tokio::sync::broadcast::channel;
//     use crate::plugin::{Args, Plugin};
//     use crate::plugins::FileInput;
//     use crate::plugins::logfmt::LogFmtParser;
//
//     #[tokio::test]
//     async fn test() {
//         let input = fs::read_to_string("../samples/fortinet_log.json").expect("Unable to open sample file");
//         let (trigger, tripwire) = Tripwire::new();
//
//
//         let args = Args::from_iter(vec![
//             ("path".to_string(), toml::Value::from("../samples/fortinet_log.json")),
//             ("parse_json".to_string(), toml::Value::from(true)),
//             ("from_beginning".to_string(), toml::Value::from(true))
//         ]);
//         let mut file = FileInput::new(args, tripwire.clone()).await.expect("Error creating FileInput");
//
//         let args = Args::from_iter(vec![("field".to_string(), toml::Value::from("message"))]);
//         let mut log_fmt = LogFmtParser::new(args, tripwire.clone()).await.expect("Couldn't create LogFmtParser");
//
//         let (sender, receiver) = channel(10);
//
//         file.connect_receiver(receiver);
//         log_fmt.connect_receiver(file.get_receiver());
//         let mut output_receiver = log_fmt.get_receiver();
//
//         tokio::spawn(async move { file.run().await } );
//         tokio::spawn(async move { log_fmt.run().await } );
//
//         let recv = output_receiver.recv().await.expect("Error receive");
//
//
//         recv.0
//     }
// }
