use std::time::Instant;


use anyhow::{anyhow, Result, Context};
use async_trait::async_trait;
use stream_cancel::{StreamExt, Tripwire};
use tokio::io::{BufWriter, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::broadcast::Receiver;

use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::{BroadcastStream};

use crate::common::logging::{debug, error, info};
use crate::duration_ms;
use crate::{Args, connect_receiver, create_event_stream, recv_event};
use crate::event::Event;
use crate::plugin::{Plugin, PluginType, ChannelType};


pub struct UnixSocketOutput {
    tripwire: Tripwire,
    socket: BufWriter<UnixStream>,
    receiver: Option<Receiver<ChannelType>>,
}

#[async_trait]
impl Plugin for UnixSocketOutput {
    fn name() -> &'static str where Self: Sized {
        "unix_socket"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> where Self: Sized {
        debug!("UnixSocketOutput args: {:#?}", args);

        // grab the path of the socket
        let file_path = args.get("path").ok_or_else(|| anyhow!("Could not find 'path' arg for UnixSocketOutput"))?;
        let file_path = file_path.as_str().ok_or_else(|| anyhow!("The 'path' arg for UnixSocketOutput does not appear to be a string"))?;
        let socket = BufWriter::new(UnixStream::connect(file_path).await
            .with_context(|| format!("opening Unix socket {}", file_path))?);

        Ok(Box::new(UnixSocketOutput {
            tripwire,
            socket,
            receiver: None, // set in connect_receiver
        }))
    }

    async fn run(&mut self) {
        let mut event_stream = create_event_stream!(self);

        let total_start = Instant::now();
        let mut start = Instant::now();
        let mut count = 0;

        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);

            // skip the None events
            if Event::None == event {
                callback.call();
                continue;
            }

            let event_str = event.to_string();

            if let Err(e) = self.socket.write_all(event_str.as_bytes()).await {
                error!("Error writing to socket: {:?}", e);
                return;
            }

            if let Err(e) = self.socket.write_all("\n".as_bytes()).await {
                error!("Error writing to socket: {:?}", e);
                return;
            }

            // call the callback
            callback.call();

            count += 1;

            if count % 100_000 == 0 {
                info!("Process rate: {:0.02}/ms", 100_000.0/(duration_ms!(start) as f64));
                start = Instant::now();
            }
        }

        if let Err(e) = self.socket.flush().await {
            error!("Error flushing socket: {:?}", e);
        }

        let secs = Instant::now().duration_since(total_start).as_secs_f64();
        info!("Took {:0.03}s to write {} lines; {}lines/sec", secs, count, (count as f64)/secs);
    }

    // boilerplate method
    connect_receiver!{}
}