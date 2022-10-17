use std::time::{Instant};

use async_trait::async_trait;
use tokio::sync::broadcast::{Receiver};
use stream_cancel::{StreamExt, Tripwire};

use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::{BroadcastStream};

use crate::common::{debug, error, info};
use crate::duration_s;
use crate::{Args, connect_receiver, create_event_stream, Plugin, recv_event};
use crate::event::Event;
use crate::plugin::{PluginType, ChannelType};

/// Output plugin for testing the speed of a Route
pub struct SpeedTest {
    tripwire: Tripwire,
    receiver: Option<Receiver<ChannelType>>,
}

#[async_trait]
impl Plugin for SpeedTest {
    fn name() -> &'static str {
        "speed_test"
    }

    async fn new(_args: Args, tripwire: Tripwire) -> anyhow::Result<Box<PluginType>> where Self: Sized {
        Ok(Box::new(SpeedTest {
            tripwire,
            receiver: None, // set in connect_receiver
        }))
    }

    async fn run(&mut self) {
        debug!("SpeedTest running...");

        let mut event_stream = create_event_stream!(self);

        let mut start = Instant::now();
        let mut count = 0;

        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);

            callback.call(); // doesn't matter where we do this

            if Event::None == event {
                continue;
            }

            count += 1;

            let secs = duration_s!(start);

            if secs > 1.0 {
                info!("{:0.03} logs/sec", (count as f64) / secs);
                count = 0;
                start = Instant::now();
            }
        }

        let secs = duration_s!(start);
        info!("{} logs/sec", (count as f64)/secs);
    }

    // boilerplate method
    connect_receiver!{}
}