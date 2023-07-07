use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono::{Local, NaiveDateTime, TimeZone, Utc};
use serde_json::{Map, Value};
use stream_cancel::{StreamExt, TakeUntilIf, Tripwire};
use syslog_loose::{parse_message, ProcId};
use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::common::logging::{debug, error, warn};
use crate::{Args, connect_receiver, create_event_stream, create_sender_semaphore, get_receiver, recv_event, send_event};
use crate::event::{Event, JsonValue};
use crate::plugin::{Plugin, PluginType, ChannelType, Callback};
use crate::plugins::syslog::parse_syslog;


pub struct FortinetParser {
    tripwire: Tripwire,
    receiver: Option<Receiver<ChannelType>>,
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    ts_field: String
}

#[async_trait]
impl Plugin for FortinetParser {
    fn name() -> &'static str where Self: Sized {
        "fortinet"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> where Self: Sized {
        debug!("SyslogParser args: {:#?}", args);

        // grab an optional timestamp field
        let ts_field = args.get("ts_field").unwrap_or(&toml::Value::String("t".to_string())).to_owned();
        let ts_field = ts_field.as_str().ok_or_else(|| anyhow!("The 'ts_field' arg for {} does not appear to be a string", Self::name()))?.to_string();

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        Ok(Box::new(FortinetParser {
            tripwire,
            receiver: None, // set in connect_receiver
            sender,
            semaphore,
            ts_field,
        }))
    }

    async fn run(&mut self) {
        debug!("FortinetParser running...");

        let mut event_stream = create_event_stream!(self);

        // grab an event and pass it along
        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);

            let event = match event {
                Event::None => {
                    continue // nothing to do here
                }
                Event::Json(_) => {
                    warn!("Found JSON log for Syslog");
                    continue;
                }
                Event::String(msg) => {
                    // strip off the beginning "pri" field
                    let json = if let Some(start) = msg.find(">") {
                        let mut date = None;
                        let mut time = None;
                        let mut map = Map::new();

                        for pair in logfmt::parse(&msg[(start+1)..]) {
                            if pair.key == "date" {
                                date = pair.val;
                                continue;
                            }

                            if pair.key == "time" {
                                time = pair.val;
                                continue;
                            }

                            if let Some(val) = pair.val {
                                map.insert(pair.key, JsonValue::from(val));
                            }
                        }

                        // attempt to stitch together a date & time
                        let date_time = if date.is_some() && time.is_some() {
                            let dt = NaiveDateTime::parse_from_str(format!("{} {}", date.unwrap(), time.unwrap()).as_str(), "%Y-%m-%d %H:%M:%S")
                                .unwrap_or(Local::now().naive_local());

                            Local.from_local_datetime(&dt)
                                .unwrap()
                        } else {
                            Local::now()
                        };

                        map.insert(self.ts_field.clone(), JsonValue::from(date_time.timestamp()));

                        map
                    } else {
                        // parse as syslog, and hope for the best :-)
                        parse_syslog(msg.as_str(), self.ts_field.as_str())
                    };

                    Event::Json(JsonValue::from(json))
                }
            };

            debug!("SENT: {:?}", event);

            // just sent along the event
            send_event!(self, event, callback);
        }

        debug!("UdpSyslogInput closing");
    }

    // boilerplate method
    get_receiver!{}
    connect_receiver!{}
}


#[cfg(test)]
mod syslog_tests {
    use crate::plugins::syslog::parse_syslog;

    #[test]
    fn test() {
        let line = r#"<190>date=2023-07-07 time=14:02:12 devname=FGT60D4Q16025343 devid=FGT60D4Q16025343 logid=1059028704 type=utm subtype=app-ctrl eventtype=app-ctrl-all level=information vd="root" appid=15895 user="" srcip=192.168.1.110 srcport=38348 srcintf="internal" dstip=74.6.231.19 dstport=443 dstintf="wan1" proto=6 service="HTTPS" policyid=1 sessionid=962 applist="default" appcat="Network.Service" app="SSL" action=pass hostname="www.yahoo.com" url="/" msg="Network.Service: SSL," apprisk=elevated"#;

        let map = parse_syslog(line, "t");

        println!("{:?}", map);
    }
}
