use std::fs;

use std::io::{SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{PathBuf};
use std::time::{Instant};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use glob::{glob, GlobError};
use inotify::{Inotify, WatchMask, EventMask};
use stream_cancel::{StreamExt, Tripwire};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, AsyncWriteExt, BufStream, BufWriter};
use tokio::select;
use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::task::JoinSet;
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::{BroadcastStream};
use toml::Value;

use crate::common::logging::{debug, error, info, warn};
use crate::{Args, connect_receiver, create_event_stream, create_sender_semaphore, get_receiver, recv_event, send_event};
use crate::event::Event;
use crate::plugin::{Plugin, PluginType, ChannelType, Callback};

// holds the state for a given file
struct FileInputInstance {
    inotify: Inotify,
    file_path: PathBuf,
    current_file: Option<BufStream<File>>,
    state_file_path: Arc<PathBuf>,
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
    tripwire: Tripwire,
    try_parse: bool, // should we try and parse as JSON
}

pub struct FileInput {
    file_instances: Vec<FileInputInstance>,
    sender: Sender<ChannelType>,
}

impl FileInputInstance {
    async fn new(file_path: PathBuf, state_file_dir: Option<PathBuf>, sender: Sender<ChannelType>, semaphore: Arc<Semaphore>, tripwire: Tripwire, from_beginning: bool, try_parse: bool) -> Result<Self> {
        if file_path.is_dir() {
            bail!("'path' argument for file is a directory, not a file. Please specify a file even if it does not yet exist");
        }

        debug!("path: {}", file_path.display());

        let dir_path = file_path.parent().ok_or_else(|| anyhow!("Cannot get the parent directory of the path: {}", file_path.display()))?;
        let file_name = file_path.file_name().unwrap().to_str().ok_or_else(|| anyhow!("Cannot get the file name of {}", file_path.display()))?;

        if !dir_path.exists() {
            bail!("The directory containing the input file does not exist");
        }

        let state_file_path = if let Some(state_dir) = state_file_dir {
            state_dir.join(format!("{}.state", file_name))
        } else {
            dir_path.join(format!("{}.state", file_name))
        };

        info!("Using state file {} for input file {}", state_file_path.display(), file_path.display());

        // 3 cases:
        // 1) We want to start from the beginning... pos = 0 & write the state file
        // 2) State file doesn't exist... pos = file's size & write state file
        // 3) State file exists... pos = state file value
        let pos = if from_beginning {
            fs::write(&state_file_path, "0").with_context(|| format!("Initializing state file: {}", state_file_path.display()))?;
            0
        } else if state_file_path.exists() {
            // otherwise, make sure we can open it, and it's value is sane
            let pos_bytes = fs::read(&state_file_path).context("Reading state file")?;
            let pos_str = String::from_utf8(pos_bytes).context("Parsing state file")?;
            pos_str.parse::<u64>().context("Parsing state file")?
        } else if file_path.exists() {
            let file_size = file_path.metadata().expect("Error getting meta data for file").size();
            fs::write(&state_file_path, format!("{}", file_size)).with_context(|| format!("Initializing state file: {}", state_file_path.display()))?;

            file_size
        } else {
            fs::write(&state_file_path, "0").with_context(|| format!("Initializing state file: {}", state_file_path.display()))?;
            0
        };

        // setup a notify on the file
        let mut inotify = Inotify::init()?;

        inotify.add_watch(dir_path, WatchMask::MODIFY | WatchMask::MOVE)
            .with_context(|| format!("Adding watch to {}", dir_path.display()))?;

        // if the file exists, open it
        let current_file = if file_path.exists() {
            let mut file = File::open(&file_path).await.with_context(|| format!("Attempting to open {}", file_path.display()))?;

            // seek to the correct position, given above
            file.seek(SeekFrom::Start(pos)).await.with_context(|| format!("Attempting to seek to the current position {} of {}", pos, file_path.display()))?;

            let pos = file.stream_position().await.unwrap();
            debug!("OPENING AT: {}", pos);

            Some(BufStream::new(file))
        } else {
            None
        };

        Ok(FileInputInstance {
            inotify,
            file_path,
            current_file,
            state_file_path: Arc::new(state_file_path),
            sender: sender.clone(),
            semaphore: semaphore.clone(),
            tripwire: tripwire.clone(),
            try_parse,
        })
    }

    /// Sends a given line down the channel, with a callback that will update the position in the state file
    async fn send_line(&mut self, line: String, pos: Option<u64>) {
        let state_file_path_clone = self.state_file_path.clone();

        // create a callback for updating the state file with this position
        let cb = Arc::new(Callback::new(move || {
            debug!("CB for: {}", state_file_path_clone.display());
            if let Some(pos) = pos {
                fs::write(state_file_path_clone.as_ref(), format!("{}", pos)).expect("Error writing to state file");
            }
        }));

        let event = if line.is_empty() {
            Event::None
        } else if self.try_parse {
            match serde_json::from_str(line.as_str()) {
                Ok(json) => Event::Json(json),
                Err(e) => {
                    warn!("Error parsing JSON: {:?}", e);

                    // count it as processed in the state file
                    cb.call();
                    return
                }
            }
        } else {
            Event::String(line)
        };

        debug!("{} sending: {:?}", self.file_path.display(), event);

        // send the event along
        send_event!(self, event, cb);
    }

    /// Reads the lines from the target file, returning the new current position
    /// to_end: should we read to the end of the file, and treat that as a line?
    async fn read_file(&mut self, mut current_pos: u64, to_end: bool) -> u64 {
        // open the file
        if self.current_file.is_none() {
            debug!("Opening file {}", self.file_path.display());

            let mut file = File::open(&self.file_path).await.with_context(|| format!("Opening {}", self.file_path.display())).expect("Error opening file");
            file.seek(SeekFrom::Start(0)).await.with_context(|| format!("Seeking file {}", self.file_path.display())).expect("Error seeking file");

            self.current_file.replace(BufStream::new(file));
        }


        loop {
            let mut line = String::with_capacity(4096);

            select! {
                amt_read_res = self.current_file.as_mut().unwrap().read_line(&mut line) => {
                    match amt_read_res {
                        Err(e) => {
                            error!("Error reading from file: {}", e);
                            return current_pos;
                        },
                        Ok(amt_read) => {
                            let amt_read = amt_read as u64;

                            if amt_read != 0 {
                                // we read a complete line
                                if line.ends_with('\n') {
                                    current_pos += amt_read;
                                    line.pop(); // remove the newline
                                    self.send_line(line, Some(current_pos)).await;
                                } else if to_end {
                                    current_pos += amt_read;
                                    self.send_line(line, Some(current_pos)).await;
                                } else {
                                    // read only to the EOF, so rewind the file
                                    self.current_file.as_mut().unwrap().seek(SeekFrom::Current(-(amt_read as i64))).await.expect("Error seeking");

                                    break
                                }
                            } else {
                                break;
                            }
                        }
                    }
                },
                _ = self.tripwire.clone() => {
                    // we're done, so just return
                    break;
                }
            };
        }

        // just return the current position
        current_pos
    }

    async fn run(&mut self) {
        debug!("FileInputInstance running: {}", self.file_path.display());

        let buffer = [0; 4096];
        let mut event_stream = self.inotify
                                   .event_stream(buffer)
                                   .expect("Error getting event stream")
                                   .take_until_if(self.tripwire.clone());

        // get the current size of the file we're watching
        let file_size = if self.file_path.exists() {
            self.file_path.metadata().expect("Unable to get the size of the file").size()
        } else {
            0
        };

        // open the state file (setup in new), and grab the current position
        let bytes = fs::read(self.state_file_path.as_ref()).expect("Unable to read state file");
        let pos_str = String::from_utf8_lossy(bytes.as_slice());
        let mut current_pos = pos_str.parse::<u64>().expect("Error parsing state file");

        debug!("CUR POS: {} FILE SIZE: {}", current_pos, file_size);

        // check if the current position is beyond the length of the file
        if current_pos > file_size {
            warn!("File is smaller than the current position");
            fs::write(self.state_file_path.as_ref(), "0").expect("Error writing to state file");
            current_pos = 0;
        }

        // check if we have unprocessed data in the file
        if file_size > current_pos {
            debug!("Reading unprocessed data");
            current_pos = self.read_file(current_pos, false).await;
        }

        // setup a cookie to track MOVE_FROM -> MOVE_TO
        let mut cookie = 0;

        let op_file_path_str = self.file_path.file_name().map(|s| s.to_os_string());

        // go through the events as we get them
        while let Some(event_res) = event_stream.next().await {
            let event = event_res.expect("Error getting event");

            match event.mask {
                EventMask::MODIFY => {
                    debug!("MODIFIED {:?}", event.name);

                    // check to see if the modify is for our target file
                    if event.name == op_file_path_str {
                        current_pos = self.read_file(current_pos, false).await;
                    }
                }
                EventMask::MOVED_TO => {
                    debug!("MOVE_TO: {:?}", event);

                    if event.cookie == cookie {
                        // try one last read from the file, grabbing whatever is left
                        self.read_file(current_pos, true).await;

                        // reset the position and set the file to None
                        current_pos = 0;
                        self.current_file.take();

                        // send a blank line with the position so the state file is updated
                        self.send_line("".to_string(), Some(0)).await;
                    }
                }
                EventMask::MOVED_FROM => {
                    debug!("MOVE_FROM: {:?}", event);

                    if event.name == op_file_path_str {
                        cookie = event.cookie;
                    }
                }
                _ => { println!("Some other kind of event: {:?}", event); }
            }
        }

    }
}

#[async_trait]
impl Plugin for FileInput {
    fn name() -> &'static str {
        "file"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> {
        debug!("FileInput args: {:#?}", args);

        // check for deprecated option
        if args.contains_key("state_file") {
            bail!("The arg 'state_file' for {} has been deprecated, please use 'state_file_dir' instead", Self::name());
        }

        // grab the optional flags for the plugin first, as they apply to all files found
        let try_parse = args.get("parse_json").unwrap_or(&Value::Boolean(false));
        let try_parse = try_parse.as_bool().ok_or_else(|| anyhow!("The 'parse_json' arg for {} does not appear to be a boolean", Self::name()))?;
        let from_beginning = args.get("from_beginning").unwrap_or(&Value::Boolean(false));
        let from_beginning = from_beginning.as_bool().ok_or_else(|| anyhow!("The 'from_beginning' arg for {} does not appear to be a boolean", Self::name()))?;

        // grab the optional state_file_dir
        let state_file_dir = match args.get("state_file_dir") {
            Some(path_val) => {
                let dir_path = PathBuf::from(path_val.as_str().ok_or_else(|| anyhow!("The 'state_file_dir' arg for {} does not appear to be a string", FileInput::name()))?);

                if !dir_path.is_dir() {
                    bail!("The path specified by 'state_file_dir' arg for {} is not a directory: {}", Self::name(), dir_path.display());
                }

                Some(dir_path)
            },
            None => None
        };

        debug!("state_file: {:?}", state_file_dir);
        debug!("parse_json: {}", try_parse);
        debug!("from_beginning: {}", from_beginning);

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args, tripwire);

        let mut file_instances = Vec::new();

        // grab the path arg, and treat it like a glob
        let file_path = args.get("path").ok_or_else(|| anyhow!("Could not find 'path' arg for {}", Self::name()))?;
        let file_path = file_path.as_str().ok_or_else(|| anyhow!("The 'path' arg for {} does not appear to be a string", Self::name()))?;
        let file_paths = glob(file_path)?.into_iter().collect::<Result<Vec<PathBuf>, GlobError>>()?;

        // if we don't have any paths, treat the arg as absolute to the file
        if file_paths.is_empty() {
            debug!("No globbing found for: {}", file_path);

            let file_path = PathBuf::from(file_path);
            let instance = FileInputInstance::new(
                file_path,
                state_file_dir,
                sender.clone(),
                semaphore.clone(),
                tripwire.clone(),
                from_beginning,
                try_parse).await?;

            file_instances.push(instance);
        } else {
            for path in file_paths.into_iter() {
                let instance = FileInputInstance::new(
                    path,
                    state_file_dir.clone(),
                    sender.clone(),
                    semaphore.clone(),
                    tripwire.clone(),
                    from_beginning,
                    try_parse).await?;

                file_instances.push(instance);
            }
        }

        debug!("Found {} instances", file_instances.len());

        Ok(Box::new(FileInput {
            file_instances,
            sender
        }))
    }

    async fn run(&mut self) {
        let mut join_set = JoinSet::new();

        // spawn each instance
        while let Some(mut instance) = self.file_instances.pop() {
            join_set.spawn(async move { instance.run().await });
        }

        // wait for them all to finish; in theory this should be fast
        while let Some(res) = join_set.join_next().await {
            if let Err(e) = res {
                error!("Error running FileInput: {}", e);
            }
        }
    }

    // boilerplate method
    get_receiver!{}
}

#[cfg(test)]
mod file_input_tests {
    use std::fs;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::Path;
    use std::time::Duration;
    use toml::Value;
    use stream_cancel::Tripwire;

    use crate::common::{debug, init_test_logger};
    use crate::{Args, FileInput, Plugin};
    use crate::event::Event;

    fn append<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) {
        let mut file = OpenOptions::new().create(true).append(true).open(path.as_ref()).expect("Error opening file");

        file.write_all(contents.as_ref()).expect("Error writing to the file");
    }

    #[tokio::test]
    async fn empty_file_write_single_line() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();
        let dir = tempfile::TempDir::new().unwrap().into_path();
        let file_path = dir.join("log");

        args.insert("channel_size".to_string(), Value::Integer(1));
        args.insert("path".to_string(), Value::String(format!("{}", file_path.display())));

        let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
        let mut recv = fi.get_receiver();

        let jh = tokio::spawn(async move { fi.run().await });

        // give the thread a chance to spawn
        tokio::time::sleep(Duration::from_millis(500)).await;

        // write a line to the file
        fs::write(&file_path, "test\n").expect("Error writing to file");

        let (event, _semaphore, callback) = recv.recv().await.expect("Error receiving");

        // make sure we got what we wrote
        assert_eq!(Event::from("test"), event);
        callback.call();

        trigger.cancel(); // stop the FileInput

        let res = jh.await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn multiple_files_write_single_line() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();
        let dir = tempfile::TempDir::new().unwrap().into_path();

        // setup 2 files
        {
            let file1 = dir.join("file1.log");
            let file2 = dir.join("file2.log");

            // write a line to the files
            fs::write(file1, "hello\n").expect("Error writing to file1");
            fs::write(file2, "world\n").expect("Error writing to file2");
        }

        args.insert("channel_size".to_string(), Value::Integer(10));
        args.insert("from_beginning".to_string(), Value::Boolean(true));
        args.insert("path".to_string(), Value::String(format!("{}/*.log", dir.display())));

        let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
        let mut recv = fi.get_receiver();

        let jh = tokio::spawn(async move { fi.run().await });

        // give the thread a chance to spawn
        tokio::time::sleep(Duration::from_millis(500)).await;

        let (event1, _semaphore, callback) = recv.recv().await.expect("Error receiving");
        callback.call();

        let (event2, _semaphore, callback) = recv.recv().await.expect("Error receiving");
        callback.call();

        // make sure we got what we wrote
        let mut events = vec![event1.to_string(), event2.to_string()];
        events.sort();

        assert_eq!("hello".to_string(), events[0]);
        assert_eq!("world".to_string(), events[1]);

        trigger.cancel(); // stop the FileInput

        let res = jh.await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn partial_line_in_file_from_beginning_write_single_line() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();
        let dir = tempfile::TempDir::new().unwrap().into_path();
        let file_path = dir.join("log");

        args.insert("channel_size".to_string(), Value::Integer(10));
        args.insert("path".to_string(), Value::String(format!("{}", file_path.display())));
        args.insert("from_beginning".to_string(), Value::Boolean(true));

        let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
        let mut recv = fi.get_receiver();

        // write a line to the file
        fs::write(&file_path, "hello ").expect("Error writing to file");

        let jh = tokio::spawn(async move { fi.run().await });

        // give the thread a chance to spawn
        tokio::time::sleep(Duration::from_millis(500)).await;

        append(&file_path, "world\n");

        let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");

        // make sure we got what we wrote
        assert_eq!(Event::from("hello world"), event);

        trigger.cancel(); // stop the FileInput

        let res = jh.await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn partial_line_in_file_from_current_write_single_line() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();
        let dir = tempfile::TempDir::new().unwrap().into_path();
        let file_path = dir.join("log");

        args.insert("channel_size".to_string(), Value::Integer(10));
        args.insert("path".to_string(), Value::String(format!("{}", file_path.display())));
        args.insert("from_beginning".to_string(), Value::Boolean(false));

        // write a line to the file BEFORE creating FileInput
        fs::write(&file_path, "hello ").expect("Error writing to file");

        let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
        let mut recv = fi.get_receiver();

        let jh = tokio::spawn(async move { fi.run().await });

        append(&file_path, "world\n");

        let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");

        // make sure we got what we wrote
        assert_eq!(Event::from("world"), event);

        trigger.cancel(); // stop the FileInput

        let res = jh.await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn partial_line_in_file_from_beginning_write_multiple_lines() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();
        let dir = tempfile::TempDir::new().unwrap().into_path();
        let file_path = dir.join("log");

        args.insert("channel_size".to_string(), Value::Integer(100));
        args.insert("path".to_string(), Value::String(format!("{}", file_path.display())));
        args.insert("from_beginning".to_string(), Value::Boolean(true));

        // write a line to the file BEFORE creating FileInput
        fs::write(&file_path, "hello ").expect("Error writing to file");

        let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
        let mut recv = fi.get_receiver();

        let jh = tokio::spawn(async move { fi.run().await });

        append(&file_path, "world\n");

        let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");

        // make sure we got what we wrote
        assert_eq!(Event::from("hello world"), event);

        // now write a bunch of lines
        for i in 0..10 {
            let mut line = format!("This is line {}\n", i);
            append(&file_path, line.as_str());

            let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");
            line.pop();
            assert_eq!(Event::String(line), event);
        }

        trigger.cancel(); // stop the FileInput

        let res = jh.await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn partial_line_in_file_from_current_write_multiple_lines() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();
        let dir = tempfile::TempDir::new().unwrap().into_path();
        let file_path = dir.join("log");

        args.insert("channel_size".to_string(), Value::Integer(100));
        args.insert("path".to_string(), Value::String(format!("{}", file_path.display())));
        args.insert("from_beginning".to_string(), Value::Boolean(false));

        // write a line to the file BEFORE creating FileInput
        fs::write(&file_path, "hello ").expect("Error writing to file");

        let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
        let mut recv = fi.get_receiver();

        let jh = tokio::spawn(async move { fi.run().await });

        append(&file_path, "world\n");

        let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");

        // make sure we got what we wrote
        assert_eq!(Event::from("world"), event);

        // now write a bunch of lines
        for i in 0..10 {
            let mut line = format!("This is line {}\n", i);
            append(&file_path, line.as_str());

            let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");
            line.pop();
            assert_eq!(Event::String(line), event);
        }

        trigger.cancel(); // stop the FileInput

        let res = jh.await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn write_multiple_lines_move_file() {
        init_test_logger();
        let (trigger, tripwire) = Tripwire::new();
        let mut args = Args::new();
        let dir = tempfile::TempDir::new().unwrap().into_path();
        let file_path = dir.join("log");

        args.insert("channel_size".to_string(), Value::Integer(100));
        args.insert("path".to_string(), Value::String(format!("{}", file_path.display())));

        let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
        let mut recv = fi.get_receiver();

        let jh = tokio::spawn(async move { fi.run().await });

        // write a bunch of lines
        for i in 0..10 {
            let mut line = format!("This is line {}\n", i);
            append(&file_path, line.as_str());

            let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");
            line.pop();
            assert_eq!(Event::String(line), event);
        }

        // move the file
        fs::rename(&file_path, format!("{}.1", file_path.display())).expect("Error renaming file");

        // make sure we get an Event::None
        let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");
        assert_eq!(Event::None, event);

        // write a bunch of lines
        for i in 0..10 {
            let mut line = format!("This is new line {}\n", i);
            append(&file_path, line.as_str());

            let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");
            line.pop();
            assert_eq!(Event::String(line), event);
        }

        // move the file
        fs::rename(&file_path, format!("{}.2", file_path.display())).expect("Error renaming file");

        // make sure we get an Event::None
        let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");
        assert_eq!(Event::None, event);

        // write a bunch of lines
        for i in 0..10 {
            let mut line = format!("This is an even newer line {}\n", i);
            append(&file_path, line.as_str());

            let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");
            line.pop();
            assert_eq!(Event::String(line), event);
        }

        trigger.cancel(); // stop the FileInput

        let res = jh.await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn reopen() {
        init_test_logger();
        let mut args = Args::new();
        let dir = tempfile::TempDir::new().unwrap().into_path();
        let file_path = dir.join("log");

        args.insert("channel_size".to_string(), Value::Integer(100));
        args.insert("path".to_string(), Value::String(format!("{}", file_path.display())));

        { // create a block so it'll close and we can re-open
            let (trigger, tripwire) = Tripwire::new();
            let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
            let mut recv = fi.get_receiver();

            let jh = tokio::spawn(async move { fi.run().await });

            // write a bunch of lines
            for i in 0..10 {
                let mut line = format!("This is line {}\n", i);
                append(&file_path, line.as_str());

                let (event, _semaphore, callback) = recv.recv().await.expect("Error receiving");
                callback.call();
                line.pop();
                assert_eq!(Event::String(line), event);
            }

            trigger.cancel(); // stop the FileInput

            let res = jh.await;

            assert!(res.is_ok());
        }

        debug!("REOPENING FILE");

        // write a bunch of lines
        for i in 0..10 {
            let line = format!("This is new line {}\n", i);
            append(&file_path, line.as_str());
        }

        { // reopen
            let (trigger, tripwire) = Tripwire::new();
            let mut fi = FileInput::new(args.clone(), tripwire.clone()).await.expect("Error creating FileInput");
            let mut recv = fi.get_receiver();

            let jh = tokio::spawn(async move { fi.run().await });

            // verify the previous lines
            for i in 0..10 {
                let mut line = format!("This is new line {}\n", i);

                let (event, _semaphore, callback) = recv.recv().await.expect("Error receiving");
                callback.call();
                line.pop();
                assert_eq!(Event::String(line), event);
            }

            // write a bunch of new lines
            for i in 0..10 {
                let mut line = format!("This is another new line {}\n", i);
                append(&file_path, line.as_str());

                let (event, _semaphore, _callback) = recv.recv().await.expect("Error receiving");
                line.pop();
                assert_eq!(Event::String(line), event);
            }

            trigger.cancel(); // stop the FileInput

            let res = jh.await;

            assert!(res.is_ok());
        }
    }

}


pub struct FileOutput {
    tripwire: Tripwire,
    file: BufWriter<File>,
    receiver: Option<Receiver<ChannelType>>,
}

#[async_trait]
impl Plugin for FileOutput {
    fn name() -> &'static str {
        "file"
    }

    async fn new(args: Args, tripwire: Tripwire) -> Result<Box<PluginType>> {
        debug!("FileOutput args: {:#?}", args);

        // grab the path of the file to read
        let file_path = args.get("path").ok_or_else(|| anyhow!("Could not find 'path' arg for FileOutput"))?;
        let file_path = file_path.as_str().ok_or_else(|| anyhow!("The 'path' arg for FileOutput does not appear to be a string"))?;
        let file = BufWriter::new(File::create(file_path).await.context(format!("Error attempting to open {}", file_path))?);

        Ok(Box::new(FileOutput {
            tripwire,
            file,
            receiver: None, // set in connect_receiver
        }))
    }

    async fn run(&mut self) {
        debug!("FileOutput running...");

        let mut event_stream = create_event_stream!(self);

        let start = Instant::now();
        let mut count = 0;

        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);
            let event_str = event.to_string();

            // debug!("FileOutput event: {}", event_str);

            if let Err(e) = self.file.write_all(event_str.as_bytes()).await {
                error!("Error writing to file: {:?}", e);
                return; // return if we can't write
            }

            // write the newline
            if let Err(e) = self.file.write_all("\n".as_bytes()).await {
                error!("Error writing to file: {:?}", e);
                return; // return if we can't write
            }

            callback.call();

            count += 1;
        }

        if let Err(e) = self.file.flush().await {
            error!("Error flushing file: {:?}", e);
        }

        let secs = Instant::now().duration_since(start).as_secs_f64();
        info!("Took {:0.03}s to write {} lines; {}lines/sec", secs, count, (count as f64)/secs);
    }

    // boilerplate method
    connect_receiver!{}
}

