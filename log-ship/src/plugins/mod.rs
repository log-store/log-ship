mod file;
mod python;
mod unix_socket;
mod insert_field;
mod insert_ts;
mod speed;
mod stdio;
mod tcp_socket;
mod journald;
mod metrics;
mod syslog;
mod lumberjack;
mod logfmt;
mod udp_socket;
mod fortinet;

pub use file::{FileInput, FileOutput};
pub use journald::JournaldInput;
pub use stdio::{StdInput, StdOutput};
pub use unix_socket::UnixSocketOutput;
pub use python::PythonScript;
pub use insert_field::InsertFieldTransform;
pub use insert_ts::InsertTimestampTransform;
pub use speed::SpeedTest;
pub use tcp_socket::TcpSocketOutput;
pub use metrics::Metrics;
pub use syslog::SyslogParser;
pub use lumberjack::LumberjackInput;
pub use crate::plugins::logfmt::LogFmtParser;
pub use udp_socket::UdpSocketInput;
pub use fortinet::FortinetParser;


// #[cfg(test)]
// mod plugins_tests {
//     use stream_cancel::Tripwire;
//     use toml::Value;
//     use common::init_test_logger;
//     use crate::Args;
//
//     #[tokio::test]
//     fn file_insert_field_and_ts_stdout() {
//         init_test_logger();
//         let (trigger, tripwire) = Tripwire::new();
//         let mut args = Args::new();
//         let dir = tempfile::TempDir::new().unwrap().into_path();
//         let file_path = dir.join("log");
//
//         args.insert("channel_size".to_string(), Value::Integer(1));
//         args.insert("path".to_string(), Value::String(format!("{}", file_path.display())));
//
//     }
//
// }