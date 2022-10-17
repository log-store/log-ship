use anyhow::{anyhow, Result, Context};
use async_trait::async_trait;
use stream_cancel::{StreamExt, Tripwire};
use tokio::io::{BufWriter, AsyncWriteExt};
use tokio::net::{TcpStream};
use tokio::sync::broadcast::Receiver;

use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::{BroadcastStream};

use crate::common::logging::{debug, error};
use crate::{Args, connect_receiver, create_event_stream, recv_event};
use crate::event::Event;
use crate::plugin::{Plugin, PluginType, ChannelType};


pub struct TcpSocketOutput {
    tripwire: Tripwire,
    tcp_stream: BufWriter<TcpStream>,
    receiver: Option<Receiver<ChannelType>>,
}

#[async_trait]
impl Plugin for TcpSocketOutput {
    fn name() -> &'static str where Self: Sized {
        "tcp_socket"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> where Self: Sized {
        debug!("TcpSocket args: {:#?}", args);

        // grab the host and port
        let host = args.get("host").ok_or_else(|| anyhow!("Could not find 'host' arg for {}", Self::name()))?;
        let host = host.as_str().ok_or_else(|| anyhow!("The 'host' arg for {} does not appear to be a string", Self::name()))?;
        let port = args.get("port").ok_or_else(|| anyhow!("Could not find 'port' arg for {}", Self::name()))?;
        let port = port.as_integer().ok_or_else(|| anyhow!("The 'port' arg for {} does not appear to be an integer", Self::name()))?;

        let tcp_stream = TcpStream::connect((host, port as u16)).await.with_context(|| format!("Connecting to remote host: {}:{}", host, port))?;
        let tcp_stream = BufWriter::new(tcp_stream);

        Ok(Box::new(TcpSocketOutput {
            tripwire,
            tcp_stream,
            receiver: None, // set in connect_receiver
        }))
    }

    async fn run(&mut self) {
        let mut event_stream = create_event_stream!(self);

        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);

            // skip the None events
            if Event::None == event {
                callback.call();
                continue;
            }

            let event_str = event.to_string();

            if let Err(e) = self.tcp_stream.write_all(event_str.as_bytes()).await {
                error!("Error writing to tcp_stream: {:?}", e);
                return;
            }

            if let Err(e) = self.tcp_stream.write_all("\n".as_bytes()).await {
                error!("Error writing to tcp_stream: {:?}", e);
                return;
            }

            // call the callback
            callback.call();
        }

        if let Err(e) = self.tcp_stream.flush().await {
            error!("Error flushing tcp_stream: {:?}", e);
        }
    }

    // boilerplate method
    connect_receiver!{}
}