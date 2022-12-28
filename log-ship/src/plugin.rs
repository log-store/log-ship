use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use stream_cancel::Tripwire;
use tokio::runtime::Handle;
use tokio::sync::broadcast::Receiver;
use tokio::sync::{OwnedSemaphorePermit};
use tokio::task;
use toml::value::Table;

use crate::event::Event;

// TODO: convert to a struct so we can more easily get arguments
pub type Args = Table;
pub type PluginType = dyn Plugin + Send + Sync;
pub type ChannelType = (Event, Arc<OwnedSemaphorePermit>, Arc<Callback>);

pub struct Callback {
    callback: Box<dyn Fn() + Send + Sync>,
}

impl Callback {
    /// Creates a new callback given the closure
    pub fn new(callback: impl Fn() + 'static + Send + Sync) -> Self {
        Callback { callback: Box::new(callback) }
    }

    /// Creates an empty no-op Callback
    pub fn empty() -> Self {
        Callback { callback: Box::new(|| {  } ) }
    }

    /// Calls the call back closure
    pub fn call(&self) {
        (self.callback)();
    }
}

impl Debug for Callback {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Callback")
    }
}


#[async_trait]
pub trait Plugin {
    /// Static method for getting the name of the plugin
    fn name() -> &'static str where Self: Sized;

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> where Self: Sized;

    fn factory() -> Box<dyn Fn(Args, Tripwire) -> Result<Box<PluginType>>> where Self: Sized {
        Box::new(|args, tripwire| {
            task::block_in_place(move || {
                Handle::current().block_on(async move {
                    Self::new(args, tripwire.clone()).await
                })
            })
        })
    }

    // TODO: Change this to returning a Result<()>
    async fn run(&mut self);

    /// Method to return the receiver of events for this plugin
    /// Input & Transform plugins *MUST* implement this method
    fn get_receiver(&self) -> Receiver<ChannelType> {
        panic!("get_receiver called on unimplemented instance");
    }

    /// Connects an upstream receiver into this plugin
    /// Transforms & Output plugins *MUST* implement this method
    fn connect_receiver(&mut self, _receiver: Receiver<ChannelType>) {
        panic!("connect_receiver called on unimplemented instance");
    }
}

// macros to make things _slightly_ easier
#[macro_export]
macro_rules! get_receiver {
    () => {
        fn get_receiver(&self) -> Receiver<ChannelType> {
            self.sender.subscribe()
        }
    }
}

#[macro_export]
macro_rules! connect_receiver {
    () => {
        fn connect_receiver(&mut self, receiver: Receiver<ChannelType>) {
            self.receiver.replace(receiver);
        }
    }
}

#[macro_export]
macro_rules! create_sender_semaphore {
    ($args:ident, $tripwire:ident) => {{
        let channel_size = $args.get("channel_size")
                               .unwrap()
                               .as_integer()
                               .map(|i| i as usize)
                               .ok_or(anyhow!("Cannot interpret 'channel_size' arg as an integer"))?;
        let (sender, _receiver) = broadcast::channel(channel_size);
        let semaphore = Arc::new(Semaphore::new(channel_size));

        let tripwire_clone = $tripwire.clone();
        let semaphore_clone = semaphore.clone();

        tokio::spawn(async move {
            tripwire_clone.await;
            semaphore_clone.close();
        });

        (sender, semaphore)
    }}
}

#[macro_export]
macro_rules! create_event_stream {
    ($self:ident) => {{
        let recv = $self.receiver.take().expect("Receiver not set");
        BroadcastStream::new(recv).take_until_if($self.tripwire.clone())
    }}
}

#[macro_export]
macro_rules! recv_event {
    ($event:ident) => {
        match $event {
            Err(e) => {
                error!("Error receiving event: {:?}", e);
                continue
            },
            // simply let the upstream permit drop so it's released
            Ok((event, _upstream_permit, callback)) => {
                (event, callback)
            }
        }
    }
}

#[macro_export]
macro_rules! send_event {
    ($self:ident, $event:ident, $callback:ident) => {
        let permit = match $self.semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return
        };

        if let Err(e) = $self.sender.send(($event, Arc::new(permit), $callback)) {
            error!("Error sending event: {:?}", e);
            return;
        }
    }
}
