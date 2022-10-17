use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::ptr::copy_nonoverlapping;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::{cmp, thread};
use std::fmt::{Debug, Formatter};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::time::Duration;

#[allow(unused_imports)]
use crate::logging::{debug, error, info, warn};

const DEFAULT_BUFFER_SIZE: usize = 4 * 1024;
const DEFAULT_WAIT_TIME: Duration = Duration::from_micros(25);

enum WriterCommand {
    Write(Vec<u8>),
    Seek(SeekFrom),
    Flush,
    Finish,
}

impl Debug for WriterCommand {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            WriterCommand::Write(b) => { write!(f, "Write({})", b.len()) }
            WriterCommand::Seek(s) => { write!(f, "See k({:?})", s)}
            WriterCommand::Flush => { write!(f, "Flush") }
            WriterCommand::Finish => { write!(f, "Finish") }
        }
    }
}

#[derive(Debug)]
pub struct DoubleBufWriter {
    sender: SyncSender<WriterCommand>,
    inner_buffer: Vec<u8>,
    is_running: Arc<AtomicBool>
}

impl DoubleBufWriter {
    pub fn new<W: 'static + Write + Seek + Send + Debug>(inner: W) -> DoubleBufWriter {
        Self::with_capacity(DEFAULT_BUFFER_SIZE, inner)
    }

    pub fn with_capacity<W: 'static + Write + Seek + Send + Debug>(capacity: usize, mut inner: W) -> DoubleBufWriter {
        let (sender, receiver) = sync_channel::<WriterCommand>(1);
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();

        // construct the thread that does the actual writing
        thread::spawn(move || {
            loop {
                match receiver.recv() {
                    Err(e) => {
                        error!("Error reading from channel, leaving loop: {:?}", e);
                        break;
                    }
                    Ok(cmd) => {
                        debug!("Got command: {:?}", cmd);
                        match cmd {
                            WriterCommand::Write(buff) => {
                                if let Err(e) = inner.write_all(buff.as_slice()) {
                                    error!("Error writing: {:?}", e);
                                    break
                                }
                            }
                            WriterCommand::Seek(from) => {
                                if let Err(e) = inner.seek(from) {
                                    error!("Error seeking: {:?}", e);
                                    break
                                }
                            }
                            WriterCommand::Flush => {
                                if let Err(e) = inner.flush() {
                                    error!("Error flushing: {:?}", e);
                                    break
                                }
                            }
                            WriterCommand::Finish => { break }
                        }
                    }
                }
            }

            debug!("Setting to false");

            // let the other side know we're done
            is_running_clone.store(false, Ordering::Relaxed);
        });

        DoubleBufWriter {
            sender,
            inner_buffer: Vec::with_capacity(capacity),
            is_running
        }
    }

    // this is here, because we cannot return the value of the position, so we can't implement Seek
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<()> {
        self.sender.send(WriterCommand::Seek(pos))
            .map_err(|e| std::io::Error::new(ErrorKind::Other, format!("{:?}", e).as_str()))
    }
}

impl Write for DoubleBufWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // compute how much of buf we can write to cur_buf
        let space_left = self.inner_buffer.capacity() - self.inner_buffer.len();
        let write_amt = cmp::min(space_left, buf.len());

        debug!("CAP: {} LEN: {} AMT: {}", self.inner_buffer.capacity(), self.inner_buffer.len(), write_amt);

        // extend our inner buffer by the correct amount
        self.inner_buffer.extend(&buf[0..write_amt]);

        // check to see if we filled the buffer
        if self.inner_buffer.len() == self.inner_buffer.capacity() {
            // grab the inner buff, replacing it with a new one with the same capacity
            let capacity = self.inner_buffer.capacity();
            let buff_to_send = std::mem::replace(&mut self.inner_buffer, Vec::with_capacity(capacity));

            // send it over the channel
            self.sender
                .send(WriterCommand::Write(buff_to_send))
                .map_err(|e| std::io::Error::new(ErrorKind::Other, format!("{:?}", e).as_str()))?;
        }

        // return how much we've written
        Ok(write_amt)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // send over whatever we have left in the buffer
        // grab the inner buff, replacing it with a new one with the same capacity
        let capacity = self.inner_buffer.capacity();
        let buff_to_send = std::mem::replace(&mut self.inner_buffer, Vec::with_capacity(capacity));

        // send it over the channel
        self.sender
            .send(WriterCommand::Write(buff_to_send))
            .map_err(|e| std::io::Error::new(ErrorKind::Other, format!("{:?}", e).as_str()))?;

        // then send the flush
        loop {
            match self.sender.try_send(WriterCommand::Flush) {
                Err(e) => {
                    match e {
                        TrySendError::Full(_) => { continue }
                        TrySendError::Disconnected(_) => {
                            error!("Error trying to flush, thread disconnected");
                            return Err(std::io::Error::new(ErrorKind::Other, "Thread disconnected"));
                        }
                    }
                }
                Ok(_) => {
                    break
                }
            }
        }

        Ok( () )
    }
}


impl Drop for DoubleBufWriter {
    fn drop(&mut self) {
        // send over whatever we have left in the buffer
        let buff_to_send = std::mem::replace(&mut self.inner_buffer, Vec::new());
        if let Err(e) = self.sender.send(WriterCommand::Write(buff_to_send)) {
            error!("Error sending Write command: {:?}", e);
        }

        // send a flush
        if let Err(e) = self.sender.send(WriterCommand::Flush) {
            error!("Error sending Flush command: {:?}", e);
        }

        // then a Finish
        if let Err(e) = self.sender.send(WriterCommand::Finish) {
            error!("Error sending Finish command: {:?}", e);
        }

        // spin until the thread is done
        while self.is_running.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_micros(10));
        }
    }
}


#[cfg(test)]
mod double_buf_tests {
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};

    use crate::init_test_logger;
    use crate::double_buf_writer::{DEFAULT_BUFFER_SIZE, DoubleBufWriter};

    #[test]
    fn open_write_read() {
        init_test_logger();

        let dir = tempfile::TempDir::new().unwrap().into_path();
        let buf = vec![0xAB; 256];
        let num_writes = (DEFAULT_BUFFER_SIZE / buf.len()) * 3;

        {
            let file = OpenOptions::new().create(true).write(true).open(dir.join("file.data")).unwrap();
            let mut file = DoubleBufWriter::new(file);

            for _ in 0..num_writes {
                file.write_all(buf.as_slice()).unwrap();
            }

            file.write_all(buf.as_slice()).unwrap();
        }

        {
            let mut file = File::open(dir.join("file.data")).unwrap();

            for _ in 0..num_writes {
                let mut read_buf = vec![0; 256];
                file.read_exact(&mut read_buf).unwrap();

                assert_eq!(buf, read_buf);
            }

            let mut read_buf = vec![0; 256];
            file.read_exact(&mut read_buf).unwrap();

            assert_eq!(buf, read_buf);
        }

    }

    #[test]
    fn open_write_flush_read() {
        init_test_logger();

        let dir = tempfile::TempDir::new().unwrap().into_path();
        let buf = vec![0xAB; 256];
        let num_writes = (DEFAULT_BUFFER_SIZE / buf.len()) * 3;
        let file = OpenOptions::new().create(true).write(true).open(dir.join("file.data")).unwrap();
        let mut write_file = DoubleBufWriter::new(file);
        let mut read_file = File::open(dir.join("file.data")).unwrap();

        for _ in 0..10 {
            write_file.write_all(buf.as_slice()).unwrap();
            write_file.flush().unwrap();

            let mut read_buf = vec![0; 256];
            read_file.read_exact(&mut read_buf).unwrap();

            assert_eq!(buf, read_buf);
        }
    }
}