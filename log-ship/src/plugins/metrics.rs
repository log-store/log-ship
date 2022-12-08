use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail};
use async_trait::async_trait;

use heim::cpu::{stats, times, usage};
use heim::memory::{memory, swap};
use heim::units::ratio;

use futures::{StreamExt as FuturesStreamExt, pin_mut};
use heim::disk::{io_counters, partitions};
use heim::net::nic;
use heim::memory::os::linux::MemoryExt;
use heim::net::os::linux::IoCountersExt;
use heim::units::time::second;
use heim::units::information::byte;
use serde_json::{Map, json};
use stream_cancel::{StreamExt, Tripwire};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, stdin, stdout};
use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::task::JoinSet;
use tokio::time::Instant;
// use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::{BroadcastStream, LinesStream};
use toml::Value;

use crate::common::logging::{debug, error, warn};
use crate::{Args, connect_receiver, create_event_stream, create_sender_semaphore, get_receiver, recv_event, send_event};
use crate::event::{Event, JsonValue};
use crate::plugin::{Plugin, PluginType, ChannelType, Callback};

pub struct Metrics {
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    tripwire: Tripwire,
    metrics: HashSet<String>,
    cpu_interval: u64,
    mem_interval: u64,
    disk_interval: u64,
    net_interval: u64
}

impl Metrics {
    async fn send_event(event: Event, semaphore: Arc<Semaphore>, sender: Sender<ChannelType>, cb: Arc<Callback>) -> anyhow::Result<()> {
        let permit = match semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return Ok( () )
        };
        if let Err(e) = sender.send((event, Arc::new(permit), cb)) {
            bail!("Error sending event: {:?}", e);
        }

        Ok( () )
    }

    async fn cpu(tripwire: Tripwire, sender: Sender<ChannelType>, semaphore: Arc<Semaphore>, interval: u64) -> anyhow::Result<()> {
        let no_op_callback = Arc::new(Callback::empty());
        let interval_duration = Duration::from_secs(interval);

        loop {
            let start = Instant::now();

            let cpu_time_stream = times().await?;
            pin_mut!(cpu_time_stream);

            let mut cpu_time_stream = cpu_time_stream
                .enumerate()
                .take_until_if(tripwire.clone()); // to short-circuit if we've been cancelled

            let mut event_map = Map::<String, JsonValue>::new();

            while let Some((i, cpu_time)) = cpu_time_stream.next().await {
                let cpu_time = cpu_time?;

                event_map.insert(format!("cpu{}.system", i), json!(cpu_time.system().get::<second>()));
                event_map.insert(format!("cpu{}.user", i), json!(cpu_time.user().get::<second>()));
                event_map.insert(format!("cpu{}.idle", i), json!(cpu_time.idle().get::<second>()));
            }

            let stats1 = stats().await?;
            let usage1 = usage().await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
            let stats2 = stats().await?;
            let usage2 = usage().await?;

            event_map.insert("ctx_switch_per_sec".to_string(), json!(stats2.ctx_switches() - stats1.ctx_switches()));
            event_map.insert("int_per_sec".to_string(), json!(stats2.interrupts() - stats1.interrupts()));

            event_map.insert("load_percent".to_string(), json!((usage2 - usage1).get::<ratio::percent>()));

            // send the event
            Metrics::send_event(Event::Json(json!(event_map)), semaphore.clone(), sender.clone(), no_op_callback.clone()).await?;

            let event_duration = Instant::now().duration_since(start);

            let wait_duration = if event_duration > interval_duration {
                warn!("Took longer to collect CPU metrics than the poll interval: {}s > {}s", event_duration.as_secs(), interval_duration.as_secs());

                // we took longer to record the metrics than we are supposed to poll!!!
                // so just wait a *very* short time to see if the tripwire has tripped
                Duration::from_micros(1)
            } else {
                // otherwise, compute how long we should wait, given how long it too to gather the info
                interval_duration - event_duration
            };

            let ret = tokio::time::timeout(wait_duration, tripwire.clone()).await;

            if ret.is_ok() {
                // tripwire has tripped, so just return
                debug!("CPU metrics finished");
                return Ok( () )
            } // otherwise, we just loop again
        }
    }

    async fn mem(tripwire: Tripwire, sender: Sender<ChannelType>, semaphore: Arc<Semaphore>, interval: u64) -> anyhow::Result<()> {
        let no_op_callback = Arc::new(Callback::empty());
        let interval_duration = Duration::from_secs(interval);

        loop {
            let start = Instant::now();
            let mut event_map = Map::<String, JsonValue>::new();

            let memory = memory().await?;
            let swap = swap().await?;

            //TODO: consider adding more
            event_map.insert("memory.free_bytes".to_string(), json!(memory.free().get::<byte>()));
            event_map.insert("memory.used_bytes".to_string(), json!(memory.used().get::<byte>()));

            event_map.insert("swap.free_bytes".to_string(), json!(swap.free().get::<byte>()));
            event_map.insert("swap.used_bytes".to_string(), json!(swap.used().get::<byte>()));

            // send the event
            Metrics::send_event(Event::Json(json!(event_map)), semaphore.clone(), sender.clone(), no_op_callback.clone()).await?;

            let event_duration = Instant::now().duration_since(start);

            let wait_duration = if event_duration > interval_duration {
                warn!("Took longer to collect CPU metrics than the poll interval: {}s > {}s", event_duration.as_secs(), interval_duration.as_secs());

                // we took longer to record the metrics than we are supposed to poll!!!
                // so just wait a *very* short time to see if the tripwire has tripped
                Duration::from_micros(1)
            } else {
                // otherwise, compute how long we should wait, given how long it too to gather the info
                interval_duration - event_duration
            };

            let ret = tokio::time::timeout(wait_duration, tripwire.clone()).await;

            if ret.is_ok() {
                // tripwire has tripped, so just return
                debug!("Memory metrics finished");
                return Ok( () )
            } // otherwise, we just loop again
        }
    }

    async fn disk(tripwire: Tripwire, sender: Sender<ChannelType>, semaphore: Arc<Semaphore>, interval: u64) -> anyhow::Result<()> {
        let no_op_callback = Arc::new(Callback::empty());
        let interval_duration = Duration::from_secs(interval);

        loop {
            let start = Instant::now();
            let mut event_map = Map::<String, JsonValue>::new();

            let mut disk_counter_values = BTreeMap::new();

            let disk_counters = io_counters().await?.take_until_if(tripwire.clone());

            futures::pin_mut!(disk_counters);

            while let Some(counter) = disk_counters.next().await {
                let counter = counter?;
                let device_name = match counter.device_name().to_str() {
                    Some(s) => s,
                    None => continue
                }.to_string();

                disk_counter_values.insert(device_name, vec![
                    counter.read_count(),
                    counter.write_count(),
                    counter.read_bytes().get::<byte>(),
                    counter.write_bytes().get::<byte>(),
                ]
                );
            }

            // now sleep for a second
            tokio::time::sleep(Duration::from_secs(1)).await;

            let disk_counters = io_counters().await?.take_until_if(tripwire.clone());

            futures::pin_mut!(disk_counters);

            while let Some(counter) = disk_counters.next().await {
                let counter = counter?;
                let device_name = match counter.device_name().to_str() {
                    Some(s) => s,
                    None => continue
                }.to_string();

                let values = match disk_counter_values.get(&device_name) {
                    Some(v) => v,
                    None => continue
                };

                event_map.clear();
                event_map.insert("device".to_string(), json!(device_name));

                event_map.insert("reads_sec".to_string(), json!(counter.read_count() - values[0]));
                event_map.insert("writes_sec".to_string(), json!(counter.write_count() - values[1]));
                event_map.insert("bytes_read_sec".to_string(), json!(counter.read_bytes().get::<byte>() - values[2]));
                event_map.insert("bytes_written_sec".to_string(), json!(counter.write_bytes().get::<byte>() - values[3]));

                // send the event
                Metrics::send_event(Event::Json(json!(event_map)), semaphore.clone(), sender.clone(), no_op_callback.clone()).await?;
            }

            let partition_stream = partitions().await?.take_until_if(tripwire.clone());

            futures::pin_mut!(partition_stream);

            while let Some(counter) = partition_stream.next().await {
                let counter = counter?;
                let mount_point = counter.mount_point().display().to_string();
                let usage = match counter.usage().await {
                    Ok(u) => u,
                    Err(_) => continue
                };

                event_map.clear();
                event_map.insert("mount_point".to_string(), json!(mount_point));

                event_map.insert("free_bytes".to_string(), json!(usage.free().get::<byte>()));
                event_map.insert("used_bytes".to_string(), json!(usage.used().get::<byte>()));

                // send the event
                Metrics::send_event(Event::Json(json!(event_map)), semaphore.clone(), sender.clone(), no_op_callback.clone()).await?;
            }

            let event_duration = Instant::now().duration_since(start);

            let wait_duration = if event_duration > interval_duration {
                warn!("Took longer to collect CPU metrics than the poll interval: {}s > {}s", event_duration.as_secs(), interval_duration.as_secs());

                // we took longer to record the metrics than we are supposed to poll!!!
                // so just wait a *very* short time to see if the tripwire has tripped
                Duration::from_micros(1)
            } else {
                // otherwise, compute how long we should wait, given how long it too to gather the info
                interval_duration - event_duration
            };

            let ret = tokio::time::timeout(wait_duration, tripwire.clone()).await;

            if ret.is_ok() {
                // tripwire has tripped, so just return
                debug!("Disk metrics finished");
                return Ok( () )
            } // otherwise, we just loop again
        }
    }

    async fn net(tripwire: Tripwire, sender: Sender<ChannelType>, semaphore: Arc<Semaphore>, interval: u64) -> anyhow::Result<()> {
        let no_op_callback = Arc::new(Callback::empty());
        let interval_duration = Duration::from_secs(interval);

        loop {
            let start = Instant::now();
            let mut event_map = Map::<String, JsonValue>::new();

            let mut net_counter_values = BTreeMap::new();
            let net_counters = heim::net::io_counters().await?.take_until_if(tripwire.clone());

            futures::pin_mut!(net_counters);

            while let Some(counter) = net_counters.next().await {
                let counter = counter?;
                let interface = counter.interface().to_string();

                net_counter_values.insert(interface, vec![
                    counter.bytes_sent().get::<byte>(),
                    counter.bytes_recv().get::<byte>(),
                    counter.packets_sent(),
                    counter.packets_recv(),
                    counter.errors_sent(),
                    counter.errors_recv(),
                    counter.drop_sent(),
                    counter.drop_recv(),
                ]);
            }

            // now sleep for a second
            tokio::time::sleep(Duration::from_secs(1)).await;

            let net_counters = heim::net::io_counters().await?.take_until_if(tripwire.clone());

            futures::pin_mut!(net_counters);

            while let Some(counter) = net_counters.next().await {
                let counter = counter?;
                let interface = counter.interface().to_string();
                let values = match net_counter_values.get(&interface) {
                    Some(v) => v,
                    None => continue
                };

                event_map.clear();
                event_map.insert("interface".to_string(), json!(interface));

                event_map.insert("bytes_sent_sec".to_string(), json!(counter.bytes_sent().get::<byte>() - values[0]));
                event_map.insert("bytes_recv_sec".to_string(), json!(counter.bytes_recv().get::<byte>() - values[1]));
                event_map.insert("packets_sent_sec".to_string(), json!(counter.packets_sent() - values[2]));
                event_map.insert("packets_recv_sec".to_string(), json!(counter.packets_recv() - values[3]));
                event_map.insert("errors_sent_sec".to_string(), json!(counter.errors_sent() - values[4]));
                event_map.insert("errors_recv_sec".to_string(), json!(counter.errors_recv() - values[5]));
                event_map.insert("drop_sent_sec".to_string(), json!(counter.drop_sent() - values[6]));
                event_map.insert("drop_recv_sec".to_string(), json!(counter.drop_recv() - values[7]));

                // send the event
                Metrics::send_event(Event::Json(json!(event_map)), semaphore.clone(), sender.clone(), no_op_callback.clone()).await?;
            }

            let event_duration = Instant::now().duration_since(start);

            let wait_duration = if event_duration > interval_duration {
                warn!("Took longer to collect CPU metrics than the poll interval: {}s > {}s", event_duration.as_secs(), interval_duration.as_secs());

                // we took longer to record the metrics than we are supposed to poll!!!
                // so just wait a *very* short time to see if the tripwire has tripped
                Duration::from_micros(1)
            } else {
                // otherwise, compute how long we should wait, given how long it too to gather the info
                interval_duration - event_duration
            };

            let ret = tokio::time::timeout(wait_duration, tripwire.clone()).await;

            if ret.is_ok() {
                // tripwire has tripped, so just return
                debug!("Net metrics finished");
                return Ok( () )
            } // otherwise, we just loop again
        }
    }
}

#[async_trait]
impl Plugin for Metrics {
    fn name() -> &'static str where Self: Sized {
        "metrics"
    }

    async fn new(args: Args, tripwire: Tripwire) -> anyhow::Result<Box<PluginType>> where Self: Sized {
        debug!("Metrics args: {:#?}", args);

        let default_metrics = vec!["cpu", "memory", "disk", "net"].into_iter().map(|s| Value::String(s.to_string())).collect::<Vec<_>>();
        let metrics = args.get("metrics").map(|a| a.clone()).unwrap_or(Value::Array(default_metrics.clone()));
        let mut metric_set = HashSet::new();

        match metrics {
            Value::String(ref s) => {
                if !default_metrics.contains(&metrics) {
                    bail!("Unknown metric {} for plugin '{}'; available metrics: cpu, memory, disk, net", s, Self::name());
                }
                metric_set.insert(s.to_owned());
            },
            Value::Array(a) => {
                for m in a.into_iter() {
                    if let Some(s) = m.as_str() {
                        if !default_metrics.contains(&m) {
                            bail!("Unknown metric {} for plugin '{}'; available metrics: cpu, memory, disk, net", s, Self::name());
                        }
                        metric_set.insert(s.to_string());
                    } else {
                        bail!("Found non-string metric for plugin '{}': {}", Self::name(), m);
                    }
                }
            },
            _ => bail!("Incorrect type for metric in plugin '{}'", Self::name())
        }

        let cpu_interval = args.get("cpu_poll_secs").unwrap_or(&Value::Integer(5));
        let cpu_interval = cpu_interval.as_integer().ok_or(anyhow!("Parameter 'cpu_poll_secs' must be an integer for plugin '{}'", Self::name()))?;

        if cpu_interval < 5 || cpu_interval > 3600 {
            bail!("Nonsensical value {} for cpu_poll_secs for plugin '{}'; should be between 5 and 3600 seconds", cpu_interval, Self::name());
        }

        let mem_interval = args.get("mem_poll_secs").unwrap_or(&Value::Integer(5));
        let mem_interval = mem_interval.as_integer().ok_or(anyhow!("Parameter 'mem_poll_secs' must be an integer for plugin '{}'", Self::name()))?;

        if mem_interval < 5 || mem_interval > 3600 {
            bail!("Nonsensical value {} for mem_poll_secs for plugin '{}'; should be between 5 and 3600 seconds", mem_interval, Self::name());
        }

        let disk_interval = args.get("disk_poll_secs").unwrap_or(&Value::Integer(30));
        let disk_interval = disk_interval.as_integer().ok_or(anyhow!("Parameter 'disk_poll_secs' must be an integer for plugin '{}'", Self::name()))?;

        if disk_interval < 5 || disk_interval > 3600 {
            bail!("Nonsensical value {} for disk_poll_secs for plugin '{}'; should be between 5 and 3600 seconds", disk_interval, Self::name());
        }

        let net_interval = args.get("net_poll_secs").unwrap_or(&Value::Integer(5));
        let net_interval = net_interval.as_integer().ok_or(anyhow!("Parameter 'net_poll_secs' must be an integer for plugin '{}'", Self::name()))?;

        if net_interval < 5 || net_interval > 3600 {
            bail!("Nonsensical value {} for net_poll_secs for plugin '{}'; should be between 5 and 3600 seconds", net_interval, Self::name());
        }

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        Ok(Box::new(Metrics {
            sender,
            semaphore,
            tripwire,
            metrics: metric_set,
            cpu_interval: cpu_interval as u64,
            mem_interval: mem_interval as u64,
            disk_interval: disk_interval as u64,
            net_interval: net_interval as u64
        }))
    }

    async fn run(&mut self) {
        let mut tasks = JoinSet::new();

        if self.metrics.contains("cpu") {
            let tripwire = self.tripwire.clone();
            let sender = self.sender.clone();
            let semaphore = self.semaphore.clone();

            tasks.spawn(Metrics::cpu(tripwire, sender, semaphore, self.cpu_interval));
        }

        if self.metrics.contains("memory") {
            let tripwire = self.tripwire.clone();
            let sender = self.sender.clone();
            let semaphore = self.semaphore.clone();

            tasks.spawn(Metrics::mem(tripwire, sender, semaphore, self.mem_interval));
        }

        if self.metrics.contains("disk") {
            let tripwire = self.tripwire.clone();
            let sender = self.sender.clone();
            let semaphore = self.semaphore.clone();

            tasks.spawn(Metrics::disk(tripwire, sender, semaphore, self.disk_interval));
        }

        if self.metrics.contains("net") {
            let tripwire = self.tripwire.clone();
            let sender = self.sender.clone();
            let semaphore = self.semaphore.clone();

            tasks.spawn(Metrics::net(tripwire, sender, semaphore, self.net_interval));
        }

        // loop through all the tasks waiting for them to finish
        while let Some(res) = tasks.join_next().await {
            match res {
                Ok(r) => {
                    if let Err(e) = r {
                        error!("Error getting metrics: {}", e)
                    }
                }
                Err(e) => {
                    error!("Error joining metrics task: {}", e);
                }
            }
        }
    }

    // boilerplate method
    get_receiver!{}
}


#[cfg(test)]
mod metrics_tests {
    use std::time::Duration;
    use stream_cancel::Tripwire;
    use toml::Value;
    use crate::common::init_test_logger;
    use crate::plugin::{Args, Plugin};
    use crate::plugins::metrics::Metrics;

    #[tokio::test]
    async fn cpu() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();

        args.insert("channel_size".to_string(), Value::Integer(1));
        args.insert("metrics".to_string(), Value::String("blah".to_string()));

        let mut metrics = Metrics::new(args.clone(), tripwire.clone()).await.expect("Error creating Metrics");
        let mut recv = metrics.get_receiver();

        let jh = tokio::spawn(async move { metrics.run().await });

        for _ in 0..100 {
            let (event, _semaphore, callback) = recv.recv().await.expect("Error receiving");

            println!("EVENT: {:?}", event);
        }

        println!("Calling cancel");
        trigger.cancel();

        jh.await.expect("Error waiting");

        // let (event, _semaphore, callback) = recv.recv().await.expect("Error receiving");
        //
        // println!("EVENT: {:?}", event);
    }
}