use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use stream_cancel::{StreamExt, TakeUntilIf, Tripwire};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::io::{AsyncReadExt, BufReader};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_util::codec::FramedRead;
use tower::Service;

use crate::common::logging::{debug, error, warn};
use crate::{Args, create_sender_semaphore, get_receiver};
use crate::event::Event;
use crate::lumberjack_decoder::LumberjackCodec;
use crate::plugin::{Plugin, PluginType, ChannelType, Callback};


/// This is _basically_ Logstash
pub struct LumberjackInput {
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    tripwire: Tripwire,
    ts_field: String,
    socket: TakeUntilIf<TcpListenerStream, Tripwire>
}

#[async_trait]
impl Plugin for LumberjackInput {
    fn name() -> &'static str where Self: Sized {
        "lumberjack"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> where Self: Sized {
        debug!("LumberjackInput args: {:#?}", args);

        // grab an optional timestamp field
        let ts_field = args.get("ts_field").unwrap_or(&toml::Value::String("t".to_string())).to_owned();
        let ts_field = ts_field.as_str().ok_or_else(|| anyhow!("The 'ts_field' arg for {} does not appear to be a string", Self::name()))?.to_string();

        // grab the host and port
        let host = args.get("host").ok_or_else(|| anyhow!("Could not find 'host' arg for {}", Self::name()))?;
        let host = host.as_str().ok_or_else(|| anyhow!("The 'host' arg for {} does not appear to be a string", Self::name()))?;
        let port = args.get("port").ok_or_else(|| anyhow!("Could not find 'port' arg for {}", Self::name()))?;
        let port = port.as_integer().ok_or_else(|| anyhow!("The 'port' arg for {} does not appear to be an integer", Self::name()))?;

        let stream = TcpListener::bind((host, port as u16)).await?;
        let socket = TcpListenerStream::new(stream).take_until_if(tripwire.clone());

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        Ok(Box::new(LumberjackInput {
            sender,
            semaphore,
            tripwire,
            ts_field,
            socket
        }))
    }

    async fn run(&mut self) {
        debug!("LumberjackInput running...");

        let no_op_callback = Arc::new(Callback::empty());

        while let Some(stream_res) = self.socket.next().await {
            let stream = match stream_res {
                Ok(stream) => stream,
                Err(e) => {
                    error!("Error getting stream: {:?}", e);
                    continue
                }
            };

            // let (stream, _addr) = self.socket.accept().await.expect("Getting connection");
            let mut reader = FramedRead::new(stream, LumberjackCodec {});

            while let Some(res) = reader.next().await {
                let res = res.expect("ERROR");

                let channel_clone = self.sender.clone();
                let semaphore_clone = self.semaphore.clone();
                let cb = no_op_callback.clone();

                tokio::spawn( async move {
                    for event in res.events {
                        let json: Value = match serde_json::from_str(event.raw.as_str()) {
                            Ok(v) => v,
                            Err(e) => {
                                error!("Error parsing JSON: {:?}", e);
                                continue
                            }
                        };

                        if json.is_object() {
                            let event = Event::Json(json);

                            let permit = match semaphore_clone.clone().acquire_owned().await {
                                Ok(p) => p,
                                Err(_) => return
                            };

                            // send down the channel
                            if let Err(e) = channel_clone.send((event, Arc::new(permit), cb.clone())) {
                                error!("Error sending event: {:?}", e);
                                return;
                            }
                        }
                    }
                });
            }

        }

        debug!("LumberjackInput closing");
    }

    // boilerplate method
    get_receiver!{}

}
