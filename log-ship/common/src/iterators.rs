use std::collections::{BTreeMap, VecDeque};
use std::fmt::Debug;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use anyhow::anyhow;
use crossbeam::queue::ArrayQueue;
use crossbeam::utils::Backoff;
use crossbeam_channel::{bounded, Receiver};


use crate::{LogEntry, LogValue};
use crate::log_entry::Field;
use crate::storage::RowId;
use crate::logging::{error};

pub struct PipelinedIterator<T> {
    // Mutex/Option that holds the value, Sender/Thread Condvar, Receiver/Iterator Condvar
    queue: Arc<ArrayQueue<T>>,
    is_done: Arc<AtomicBool>
}

impl <T: 'static + Debug + Send> PipelinedIterator<T> {
    pub fn new<I: 'static + Iterator<Item=T> + Send>(mut iter: I) -> Self {
        let queue = Arc::new(ArrayQueue::new(1000));
        let queue_clone = queue.clone();

        let is_done = Arc::new(AtomicBool::new(false));
        let is_done_clone = is_done.clone();

        thread::spawn(move || {
            let backoff = Backoff::new();

            while let Some(next) = iter.next() {
                // debug!("THREAD: have next, grabbing lock: {:?}", next);

                let mut ret = queue_clone.push(next);
                // let mut spin_count = 1;

                while let Err(next) = ret {
                    // println!("SPUN: {}", spin_count);
                    // spin_count += 1;
                    backoff.spin();
                    ret = queue_clone.push(next);
                }


                // backoff.reset();
            }

            // debug!("THREAD: we're done, flipping-the-flag");
            // upstream iterator is done
            is_done_clone.store(true, Ordering::Relaxed);
        });

        PipelinedIterator {
            queue,
            is_done
        }
    }
}

impl <T: Debug> Iterator for PipelinedIterator<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let backoff = Backoff::new();
        let mut ret = self.queue.pop();

        while ret.is_none() {
            // check to see if we're done
            if self.is_done.load(Ordering::Relaxed) {
                return None;
            }

            backoff.spin();

            ret = self.queue.pop();
        }

        ret // we know this is Some(T)
    }
}


/// An Iterator that deduplicates runs of values
struct DedupRuns<I> {
    it: I,
    cur_val: RowId,
    cur_count: usize,
    target_count: usize
}

impl <I: Iterator<Item=RowId>> DedupRuns<I> {
    pub fn new(mut it: I, target_count: usize) -> Self {
        let cur_val = it.next().unwrap();

        DedupRuns {
            it,
            cur_val,
            cur_count: 1,
            target_count
        }
    }
}

impl <I: Iterator<Item=RowId>> Iterator for DedupRuns<I> {
    type Item = RowId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let val = self.it.next()?;

            if self.cur_val == val {
                self.cur_count += 1;
            } else {
                self.cur_val = val;
                self.cur_count = 1;
            }

            if self.cur_count == self.target_count {
                return Some(val)
            }
        }
    }
}


pub struct LogEntryFold<I> {
    iterators: Vec<I>,
    cur_entries: VecDeque<(RowId, LogEntry)>,
    is_done: bool
}

impl <I: Iterator<Item=anyhow::Result<(RowId, Field, LogValue)>>> LogEntryFold<I> {
    pub fn new(iterators: Vec<I>) -> Self {
        let iters_len = iterators.len();

        LogEntryFold {
            iterators,
            cur_entries: VecDeque::with_capacity(iters_len),
            is_done: false
        }
    }
}

impl <I: Iterator<Item=anyhow::Result<(RowId, Field, LogValue)>>> Iterator for LogEntryFold<I> {
    type Item = anyhow::Result<LogEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        // first check to see if we're done
        if self.is_done {
            return None;
        }

        let le_len = self.iterators.len();

        for iter in self.iterators.iter_mut() {
            if let Some(res) = iter.next() {
                let (row_id, field, value) = match res {
                    Err(e) => return Some(Err(e)),
                    Ok(row) => row
                };

                match self.cur_entries.binary_search_by_key(&row_id, |(id, _le)| *id) {
                    Ok(idx) => { self.cur_entries[idx].1.insert(field, value); },
                    Err(idx) => {
                        let mut le = LogEntry::with_capacity(le_len);

                        le.insert(field, value);

                        self.cur_entries.insert(idx, (row_id, le));
                    }
                }
            }
        }

        if let Some((_id, le)) = self.cur_entries.pop_back() {
            Some(Ok(le))
        } else {
            self.is_done = true;
            None
        }
    }
}


pub struct ThreadedLogEntryFold {
    receivers: Vec<Receiver<anyhow::Result<(RowId, Arc<str>, LogValue)>>>,
    cur_entries: BTreeMap<RowId, LogEntry>,
    is_done: bool
}

impl ThreadedLogEntryFold {
    pub fn new<I: 'static + Iterator<Item=anyhow::Result<(RowId, Arc<str>, LogValue)>> + Send>(iterators: Vec<I>) -> Self {
        // create a channel, and thread for each iterator
        let mut receivers = Vec::with_capacity(iterators.len());

        for mut iter in iterators {
            let (sender, receiver) = bounded(2);
            receivers.push(receiver);

            // TODO: Capture handles
            thread::spawn(move || {
                while let Some(row) = iter.next() {
                    // debug!("Writing Row: {:?}", row);
                    if let Err(e) = sender.send(row) {
                        error!("Error sending: {:?}", e);
                        break
                    }
                }
            });
        }

        ThreadedLogEntryFold {
            receivers,
            cur_entries: BTreeMap::new(),
            is_done: false
        }
    }
}

impl Iterator for ThreadedLogEntryFold {
    type Item = anyhow::Result<LogEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        // first check to see if we're done
        if self.is_done {
            return None;
        }

        // go through the iterator channels
        for recv in self.receivers.iter() {
            // assume errors mean that channel is done
            let res = match recv.recv() {
                Err(_e) => continue,
                Ok(res) => res
            };

            // handle an error from the underlying iterator
            let (row_id, field, value) = match res {
                Err(e) => return Some(Err(anyhow!(e))),
                Ok(row) => row
            };

            // get the LogEntry for the column, constructing it if we don't already have it
            let le = self.cur_entries.entry(row_id).or_insert(LogEntry::with_capacity(self.receivers.len()));

            // insert the field & value into the entry
            le.insert(Field::from(field.as_ref()), value);
        }

        // grab the LogEntry with the highest RowId
        if let Some(row_id) = self.cur_entries.keys().last().cloned() {
            self.cur_entries.remove(&row_id).map(|le| Ok(le))
        } else {
            self.is_done = true;
            None
        }
    }
}


#[cfg(test)]
mod piplined_iterator_tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::{duration_us, init_test_logger};
    use crate::iterators::PipelinedIterator;
    use crate::logging::debug;

    #[test]
    fn test() {
        init_test_logger();

        let v = vec![1, 2, 3, 4u64];
        let it: Box<dyn Iterator<Item=u64> + Send> = Box::new(v.into_iter());
        let mut it = PipelinedIterator::new(it);

        assert_eq!(Some(1), it.next());
        assert_eq!(Some(2), it.next());
        assert_eq!(Some(3), it.next());
        assert_eq!(Some(4), it.next());
        assert_eq!(None, it.next());
    }

    #[test]
    fn timing_test() {
        init_test_logger();

        let v = (0..100_000u64).into_iter().collect::<Vec<_>>();

        let upstream_wait = 30u64;
        let process_wait = 15u64;

        // make an iterator that waits for 30us
        let upstream_iter: Box<dyn Iterator<Item=u64> + Send> = Box::new(
            v.iter().cloned()
                .map(move |v| {
                    thread::sleep(Duration::from_micros(upstream_wait));
                    v
                })
        );

        // make another that takes 10us to process
        let it: Box<dyn Iterator<Item=u64> + Send> = Box::new(
            upstream_iter.map(|v| {
                    thread::sleep(Duration::from_micros(process_wait));
                    v
                })
        );

        // time how long it takes to run the whole thing
        let start = Instant::now();
        it.count();
        let full_duration = duration_us!(start);

        // now put our PipelinedIterator in there to see if it is faster
        let mut upstream_iter: Box<dyn Iterator<Item=u64> + Send> = Box::new(
            v.into_iter()
             .map(move |v| {
                 thread::sleep(Duration::from_micros(upstream_wait));
                 v
             })
        );

        upstream_iter = Box::new(PipelinedIterator::new(upstream_iter));

        let it: Box<dyn Iterator<Item=u64> + Send> = Box::new(
            upstream_iter.map(|v| {
                thread::sleep(Duration::from_micros(process_wait));
                v
            })
        );

        // time how long it takes to run the whole thing
        let start = Instant::now();
        it.count();
        let pipelined_duration = duration_us!(start);

        debug!("FULL: {}\tPIPELINE: {}", full_duration, pipelined_duration);

        assert!(pipelined_duration < full_duration);
    }
}


#[cfg(test)]
mod logentry_fold_tests {
    use std::sync::Arc;
    use crate::{ThreadedLogEntryFold, LogValue, init_test_logger, LogEntry};

    #[test]
    fn equal_lengths() {
        init_test_logger();
        let iterators = vec![
            vec![
                Ok((10, Arc::from("r"), LogValue::from("r-field-10"))),
                Ok((9, Arc::from("r"), LogValue::from("r-field-9"))),
                Ok((8, Arc::from("r"), LogValue::from("r-field-8"))),
            ].into_iter(),

            vec![
                Ok((10, Arc::from("s"), LogValue::from("s-field-10"))),
                Ok((9, Arc::from("s"), LogValue::from("s-field-9"))),
                Ok((8, Arc::from("s"), LogValue::from("s-field-8"))),
            ].into_iter(),

            vec![
                Ok((10, Arc::from("t"), LogValue::from("t-field-10"))),
                Ok((9, Arc::from("t"), LogValue::from("t-field-9"))),
                Ok((8, Arc::from("t"), LogValue::from("t-field-8"))),
            ].into_iter(),
        ];
        let expected = vec![
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-10")), ("s", LogValue::from("s-field-10")), ("t", LogValue::from("t-field-10"))].into_iter()),
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-9")), ("s", LogValue::from("s-field-9")), ("t", LogValue::from("t-field-9"))].into_iter()),
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-8")), ("s", LogValue::from("s-field-8")), ("t", LogValue::from("t-field-8"))].into_iter()),
        ];

        let it = ThreadedLogEntryFold::new(iterators);

        for (res, exp) in it.zip(expected.into_iter()) {
            assert!(res.is_ok());

            let le = res.unwrap();
            assert_eq!(le, exp);
        }
    }

    #[test]
    fn unequal_lengths() {
        init_test_logger();
        let iterators = vec![
            vec![
                Ok((10, Arc::from("r"), LogValue::from("r-field-10"))),
                Ok((9, Arc::from("r"), LogValue::from("r-field-9"))),
                Ok((8, Arc::from("r"), LogValue::from("r-field-8"))),
            ].into_iter(),

            vec![
                Ok((10, Arc::from("s"), LogValue::from("s-field-10"))),
                Ok((9, Arc::from("s"), LogValue::from("s-field-9"))),
            ].into_iter(),

            vec![
                Ok((9, Arc::from("t"), LogValue::from("t-field-9"))),
            ].into_iter(),
        ];
        let expected = vec![
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-10")), ("s", LogValue::from("s-field-10"))].into_iter()),
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-9")), ("s", LogValue::from("s-field-9")), ("t", LogValue::from("t-field-9"))].into_iter()),
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-8"))].into_iter()),
        ];

        let it = ThreadedLogEntryFold::new(iterators);

        for (res, exp) in it.zip(expected.into_iter()) {
            assert!(res.is_ok());

            let le = res.unwrap();
            assert_eq!(le, exp);
        }
    }

    #[test]
    fn nonmatching_rows() {
        init_test_logger();
        let iterators = vec![
            vec![
                Ok((10, Arc::from("r"), LogValue::from("r-field-10"))),
                Ok((8, Arc::from("r"), LogValue::from("r-field-8"))),
            ].into_iter(),

            vec![
                Ok((10, Arc::from("s"), LogValue::from("s-field-10"))),
                Ok((9, Arc::from("s"), LogValue::from("s-field-9"))),
            ].into_iter(),

            vec![
                Ok((9, Arc::from("t"), LogValue::from("t-field-9"))),
                Ok((8, Arc::from("t"), LogValue::from("t-field-8"))),
            ].into_iter(),
        ];
        let expected = vec![
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-10")), ("s", LogValue::from("s-field-10"))].into_iter()),
            LogEntry::from_iter(vec![("s", LogValue::from("s-field-9")), ("t", LogValue::from("t-field-9"))].into_iter()),
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-8")), ("t", LogValue::from("t-field-8"))].into_iter()),
        ];

        let it = ThreadedLogEntryFold::new(iterators);

        for (res, exp) in it.zip(expected.into_iter()) {
            assert!(res.is_ok());

            let le = res.unwrap();
            assert_eq!(le, exp);
        }
    }

    #[test]
    fn one_empty() {
        init_test_logger();
        let iterators = vec![
            vec![
                Ok((10, Arc::from("r"), LogValue::from("r-field-10"))),
                Ok((9, Arc::from("r"), LogValue::from("r-field-9"))),
                Ok((8, Arc::from("r"), LogValue::from("r-field-8"))),
            ].into_iter(),

            vec![
            ].into_iter(),

            vec![
                Ok((10, Arc::from("t"), LogValue::from("t-field-10"))),
                Ok((9, Arc::from("t"), LogValue::from("t-field-9"))),
                Ok((8, Arc::from("t"), LogValue::from("t-field-8"))),
            ].into_iter(),
        ];
        let expected = vec![
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-10")), ("t", LogValue::from("t-field-10"))].into_iter()),
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-9")), ("t", LogValue::from("t-field-9"))].into_iter()),
            LogEntry::from_iter(vec![("r", LogValue::from("r-field-8")), ("t", LogValue::from("t-field-8"))].into_iter()),
        ];

        let it = ThreadedLogEntryFold::new(iterators);

        for (res, exp) in it.zip(expected.into_iter()) {
            assert!(res.is_ok());

            let le = res.unwrap();
            assert_eq!(le, exp);
        }
    }

    #[test]
    fn all_empty() {
        init_test_logger();
        let iterators = vec![
            vec![
            ].into_iter(),

            vec![
            ].into_iter(),

            vec![
            ].into_iter(),
        ];
        let expected = vec![
        ];

        let it = ThreadedLogEntryFold::new(iterators);

        for (res, exp) in it.zip(expected.into_iter()) {
            assert!(res.is_ok());

            let le = res.unwrap();
            assert_eq!(le, exp);
        }
    }

}