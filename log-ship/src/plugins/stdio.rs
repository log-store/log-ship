use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use stream_cancel::{StreamExt, Tripwire};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, stdin, stdout};
use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::{BroadcastStream, LinesStream};
use toml::Value;

use crate::common::logging::{debug, error, warn};
use crate::{Args, connect_receiver, create_event_stream, create_sender_semaphore, get_receiver, recv_event, send_event};
use crate::event::Event;
use crate::plugin::{Plugin, PluginType, ChannelType, Callback};


pub struct StdInput {
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    tripwire: Tripwire,
    try_parse: bool, // should we try and parse as JSON
}

#[async_trait]
impl Plugin for StdInput {
    fn name() -> &'static str where Self: Sized {
        "stdin"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> where Self: Sized {
        debug!("StdInput args: {:#?}", args);

        // grab the optional flags for the plugin
        let try_parse = args.get("parse_json").unwrap_or(&Value::Boolean(false));
        let try_parse = try_parse.as_bool().ok_or(anyhow!("The 'parse_json' arg for {} does not appear to be a boolean", Self::name()))?;

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args);

        Ok(Box::new(StdInput {
            sender,
            semaphore,
            tripwire,
            try_parse
        }))
    }

    async fn run(&mut self) {
        debug!("StdInput running...");

        // create the event stream
        let lines = BufReader::new(stdin()).lines();
        let mut line_stream = LinesStream::new(lines)
            .take_until_if(self.tripwire.clone());
        let no_op_callback = Arc::new(Callback::empty());

        // go through the lines
        while let Some(line_res) = line_stream.next().await {
            let line = line_res.expect("Error reading line from STDIN");

            let event = if self.try_parse {
                match serde_json::from_str(line.as_str()) {
                    Ok(json) => Event::Json(json),
                    Err(_e) => {
                        warn!("Could not parse line as JSON: {}", line);
                        continue
                    }
                }
            } else {
                Event::String(line)
            };

            // send the event along
            let cb = no_op_callback.clone();
            send_event!(self, event, cb);
        }

        debug!("StdInput closing");
    }

    // boilerplate method
    get_receiver!{}
}

pub struct StdOutput {
    tripwire: Tripwire,
    receiver: Option<Receiver<ChannelType>>,
}

#[async_trait]
impl Plugin for StdOutput {
    fn name() -> &'static str {
        "stdout"
    }

    async fn new(_args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> {
        Ok(Box::new(StdOutput {
            tripwire,
            receiver: None, // set in connect_receiver
        }))
    }

    async fn run(&mut self) {
        debug!("StdOutput running...");

        let mut event_stream = create_event_stream!(self);

        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);

            if Event::None == event {
                callback.call();
                continue
            }

            let line = event.to_string() + "\n";

            // write the line, and call the callback
            stdout().write_all(line.as_bytes()).await.expect("Error writing to STDOUT");
            callback.call();
        }

        stdout().flush().await.expect("Error flushing STDOUT");
        debug!("StdOutput closing");
    }

    // boilerplate method
    connect_receiver!{}
}

