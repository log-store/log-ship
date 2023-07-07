use std::sync::Arc;
use anyhow::{anyhow, Result, Context};
use async_trait::async_trait;
use stream_cancel::{StreamExt, TakeUntilIf, Tripwire};
use tokio::net::{UdpSocket};
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_util::codec::LinesCodec;
use tokio_util::udp::UdpFramed;

use crate::common::logging::{debug, error};
use crate::{Args, connect_receiver, create_event_stream, create_sender_semaphore, get_receiver, recv_event};
use crate::event::Event;
use crate::plugin::{Plugin, PluginType, ChannelType, Callback};


pub struct UdpSocketInput {
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    udp_stream: TakeUntilIf<UdpFramed<LinesCodec>, Tripwire>,
    tripwire: Tripwire,
    try_parse: bool
}

#[async_trait]
impl Plugin for UdpSocketInput {
    fn name() -> &'static str where Self: Sized {
        "udp_socket"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> where Self: Sized {
        debug!("UdpSocketInput args: {:#?}", args);

        // see if we should try and parse as JSON
        let try_parse = args.get("parse_json").unwrap_or(&toml::Value::Boolean(false));
        let try_parse = try_parse.as_bool().ok_or_else(|| anyhow!("The 'parse_json' arg for {} does not appear to be a boolean", Self::name()))?;

        // grab the host and port
        let host = args.get("host").ok_or_else(|| anyhow!("Could not find 'host' arg for {}", Self::name()))?;
        let host = host.as_str().ok_or_else(|| anyhow!("The 'host' arg for {} does not appear to be a string", Self::name()))?;
        let port = args.get("port").ok_or_else(|| anyhow!("Could not find 'port' arg for {}", Self::name()))?;
        let port = port.as_integer().ok_or_else(|| anyhow!("The 'port' arg for {} does not appear to be an integer", Self::name()))?;

        let udp_stream = UdpSocket::bind((host, port as u16)).await.with_context(|| format!("Connecting to remote host: {}:{}", host, port))?;
        let udp_stream = UdpFramed::new(udp_stream, LinesCodec::new());
        let udp_stream = udp_stream.take_until_if(tripwire.clone());

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        Ok(Box::new(UdpSocketInput {
            sender,
            semaphore,
            udp_stream,
            tripwire,
            try_parse,
        }))
    }

    async fn run(&mut self) {
        debug!("UdpSocketInput running...");

        let no_op_callback = Arc::new(Callback::empty());

        // go through the lines
        while let Some(res) = self.udp_stream.next().await {
            match res {
                Ok((line, socket)) => {
                    let channel_clone = self.sender.clone();
                    let semaphore_clone = self.semaphore.clone();
                    let cb = no_op_callback.clone();
                    let permit = match semaphore_clone.acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => return
                    };

                    // do a spawn so we're not blocking while parsing syslog
                    tokio::spawn(async move {
                        // attempt to parse into JSON
                        let json = match serde_json::from_str(line.as_str()) {
                            Ok(j) => j,
                            Err(e) => {
                                error!("Error parsing JSON: {:?}", e);
                                return;
                            }
                        };

                        let event = Event::Json(json);

                        // send down the channel
                        if let Err(e) = channel_clone.send((event, Arc::new(permit), cb)) {
                            error!("Error sending event: {:?}", e);
                            return;
                        }
                    });
                }
                Err(e) => {
                    // be robust here, just log and keep going
                    error!("Error reading from stream"; "error" => e.to_string());
                }
            }
        }

        debug!("UdpSocketInput closing");
    }

    // boilerplate method
    get_receiver!{}
}