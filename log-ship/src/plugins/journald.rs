use std::{fs, mem};
use std::path::PathBuf;
use std::sync::{Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use systemd::{journal, JournalSeek};
use stream_cancel::{StreamExt, Tripwire};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{broadcast, Semaphore};
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::UnboundedReceiverStream;
use toml::Value;

use crate::common::logging::{debug, error};
use crate::{create_sender_semaphore, get_receiver, send_event};
use crate::event::Event;
use crate::plugin::{Args, Callback, ChannelType, Plugin, PluginType};

pub struct JournaldInput {
    journal_type: String,
    from_beginning: bool,
    cursor_file_path: PathBuf,
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    tripwire: Tripwire,
}

#[async_trait]
impl Plugin for JournaldInput {
    fn name() -> &'static str where Self: Sized {
        "journald"
    }

    async fn new(args: Args, tripwire: Tripwire) -> anyhow::Result<Box<PluginType>> where Self: Sized {
        // which journal to open: system, user, all (default)
        let journal_type = args.get("journal").map(|v| v.to_owned()).unwrap_or(Value::String("all".to_string()));
        let journal_type = journal_type.as_str().ok_or(anyhow!("The 'journal' arg for {} does not appear to be a string", Self::name()))?;
        debug!("Journal Type: {}", journal_type);

        match journal_type {
            "all"|"system"|"user" => (),
            _ => bail!("Unknown journal '{}', please leave blank or using one of 'system' or 'user'", journal_type)
        }

        // read from the beginning?
        let from_beginning = args.get("from_beginning").unwrap_or(&Value::Boolean(false));
        let from_beginning = from_beginning.as_bool().ok_or(anyhow!("The 'from_beginning' arg for {} does not appear to be a boolean", Self::name()))?;
        debug!("From beginning: {}", from_beginning);

        // grab an cursor file
        let cursor_file_path = args.get("cursor_file").ok_or(anyhow!("Could not find 'cursor_file' arg for {}", Self::name()))?;
        let cursor_file_path = cursor_file_path.as_str().ok_or(anyhow!("The 'cursor_file' arg for {} does not appear to be a string", Self::name()))?;
        let cursor_file_path = PathBuf::from(cursor_file_path.to_string());

        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        Ok(Box::new(JournaldInput {
            journal_type: journal_type.to_string(),
            from_beginning,
            cursor_file_path,
            sender,
            semaphore,
            tripwire
        }))
    }

    async fn run(&mut self) {
        let running = Arc::new(AtomicBool::new(true));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let journal_type = mem::take(&mut self.journal_type);
        let from_beginning = self.from_beginning;

        let cursor_file_path_clone = self.cursor_file_path.clone();
        let running_clone = running.clone();

        tokio::task::spawn_blocking(move || {
            // have to make the Journal in this thread
            let mut journal = journal::OpenOptions::default()
                .all_namespaces(if journal_type == "all" { true } else { false })
                .system(if journal_type == "system" { true } else { false })
                .current_user(if journal_type == "user" { true } else { false })
                .open().expect("Error opening journal");

            if from_beginning {
                journal.seek(JournalSeek::Head).expect("Error seeking");
            } else if cursor_file_path_clone.exists() {
                // try to open the cursor file, and read in that value
                let cursor = fs::read_to_string(cursor_file_path_clone).expect("Error reading from cursor file");

                journal.seek_cursor(cursor).expect("Error seeking");
            }

            // first get any entries that already exist
            // see: https://www.freedesktop.org/software/systemd/man/sd_journal_wait.html#Examples
            let mut await_entry = false;

            while running_clone.load(Ordering::Relaxed) {
                let op_entry = if await_entry {
                    journal.await_next_entry(Some(Duration::from_micros(100)))
                } else {
                    journal.next_entry()
                }.expect("Error getting next journald entry");

                if op_entry.is_none() && !await_entry {
                    debug!("Awaiting new entries");
                    await_entry = true;
                }

                if let Some(entry) = op_entry {
                    let map = entry.into_iter()
                                   .map(|(k, v)| (k, serde_json::Value::String(v)))
                                   .collect::<serde_json::Map<String, serde_json::Value>>();

                    let event = Event::Json(serde_json::Value::Object(map));

                    // grab the cursor
                    let cursor = journal.cursor().unwrap();

                    tx.send((event, cursor)).expect("Error sending on channel");
                }
            }
        });

        let mut recv_stream = UnboundedReceiverStream::new(rx).take_until_if(self.tripwire.clone());
        let cursor_file_path = Arc::new(mem::take(&mut self.cursor_file_path));

        while let Some((event, cursor)) = recv_stream.next().await {
            let cursor_file_path_clone = cursor_file_path.clone();

            let callback = Arc::new(Callback::new(move || {
                fs::write(cursor_file_path_clone.as_ref(), format!("{}", cursor))
                    .expect("Error writing to cursor file");
            }));

            // send the event along
            send_event!(self, event, callback);
        }

        // stop our loop above
        running.store(false, Ordering::Relaxed);
    }

    // boilerplate method
    get_receiver!{}
}

#[cfg(test)]
mod journald_input_tests {
    use stream_cancel::Tripwire;
    use toml::Value;

    use crate::common::init_test_logger;
    use crate::plugin::{Args, Plugin};
    use crate::plugins::journald::JournaldInput;

    #[tokio::test]
    async fn new() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();

        args.insert("channel_size".to_string(), Value::Integer(1));

        JournaldInput::new(args, tripwire).await.expect("Error creating JournaldInput");
    }
}