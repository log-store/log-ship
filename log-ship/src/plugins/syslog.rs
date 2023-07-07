use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
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


/// Attempt to convert a Syslog message into JSON
// pub(crate) fn parse_syslog(syslog_message: &str, socket_hostname: String, timestamp_field: &str) -> Map<String, Value> {
pub fn parse_syslog(syslog_message: &str, timestamp_field: &str) -> Map<String, Value> {
    // parse the syslog message
    let message = parse_message(syslog_message);

    debug!("Syslog:\n\t{}\n\t{}", syslog_message, message);

    // construct the JSON from the message
    let mut value = Map::new();

    if let Some(ts) = message.timestamp {
        value.insert(timestamp_field.to_string(), Value::from(ts.timestamp_millis()));
    } else {
        value.insert(timestamp_field.to_string(), Value::from(Utc::now().timestamp_millis()));
    }

    if let Some(app_name) = message.appname {
        value.insert("app_name".to_string(), Value::from(app_name));
    }

    if let Some(facility) = message.facility {
        value.insert("facility".to_string(), Value::from(facility.as_str()));
    }

    if let Some(msg_id) = message.msgid {
        value.insert("msg_id".to_string(), Value::from(msg_id));
    }

    if let Some(hostname) = message.hostname {
        value.insert("hostname".to_string(), Value::from(hostname));
    // } else {
    //     value.insert("hostname".to_string(), Value::from(socket_hostname));
    }

    if let Some(proc_id) = message.procid {
        match proc_id {
            ProcId::PID(pid) => {
                value.insert("proc_id".to_string(), Value::from(pid));
            }
            ProcId::Name(name) => {
                value.insert("proc_id".to_string(), Value::from(name));
            }
        }
    }

    if let Some(severity) = message.severity {
        value.insert("severity".to_string(), Value::from(severity.as_str()));
    }

    for sd in message.structured_data.iter() {
        if !sd.id.is_empty() {
            value.insert("id".to_string(), Value::from(sd.id));
        }

        for (k, v) in sd.params.iter() {
            value.insert(k.to_string(), Value::from(*v));
        }
    }

    // now get the message field, and attempt to parse it as JSON
    if let Ok(json) = serde_json::de::from_str::<Value>(message.msg) {
        // if we have an object, insert the keys and values
        if json.is_object() {
            for (k, v) in json.as_object().unwrap() {
                value.insert(k.clone(), v.clone());
            }
        } else {
            // otherwise insert whatever it is
            value.insert("+message".to_string(), json);
        }
    } else {
        // insert parsing all pieces
        value.insert("+message".to_string(), Value::from(message.msg));
    }

    value
}


pub struct SyslogParser {
    tripwire: Tripwire,
    receiver: Option<Receiver<ChannelType>>,
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    ts_field: String
}

#[async_trait]
impl Plugin for SyslogParser {
    fn name() -> &'static str where Self: Sized {
        "syslog"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> where Self: Sized {
        debug!("SyslogParser args: {:#?}", args);

        // grab an optional timestamp field
        let ts_field = args.get("ts_field").unwrap_or(&toml::Value::String("t".to_string())).to_owned();
        let ts_field = ts_field.as_str().ok_or_else(|| anyhow!("The 'ts_field' arg for {} does not appear to be a string", Self::name()))?.to_string();

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        Ok(Box::new(SyslogParser {
            tripwire,
            receiver: None, // set in connect_receiver
            sender,
            semaphore,
            ts_field,
        }))
    }

    async fn run(&mut self) {
        debug!("SyslogParser running...");

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
                    let json = parse_syslog(msg.as_str(), self.ts_field.as_str());
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
