//! Used to setup logging to just get it out of main

use std::io::Write;
use slog::{Drain, Filter, Logger, o};
use slog_scope::GlobalLoggerGuard;

// re-export these
pub use slog::FilterLevel;
use slog_async::OverflowStrategy;
pub use slog_scope::{debug, error, info, warn, trace};

#[allow(dead_code)]
pub fn setup() -> GlobalLoggerGuard {
    setup_with_level(FilterLevel::Info)
}

pub fn setup_with_level(level: FilterLevel) -> GlobalLoggerGuard {
    setup_with_level_location(level, std::io::stdout())
}

pub fn setup_with_level_location<W: 'static + Write + Send>(level: FilterLevel, location: W) -> GlobalLoggerGuard {
    // use a sync logger in debug mode
    if level == FilterLevel::Debug {
        let decorator = slog_term::PlainSyncDecorator::new(location);
        let mut format_builder = slog_term::FullFormat::new(decorator).use_local_timestamp();
        format_builder = format_builder.use_file_location(); // show file and location when debugging

        let drain = format_builder.build().fuse();
        let drain = Filter::new(drain, move |r| level.accepts(r.level())).fuse();
        let logger = Logger::root(drain, o!());

        slog_scope::set_global_logger(logger)
    } else {
        let decorator = slog_term::PlainDecorator::new(location);
        let format_builder = slog_term::FullFormat::new(decorator).use_local_timestamp();

        let drain: Box<dyn Drain<Err=std::io::Error, Ok=_> + Send> = Box::new(format_builder.build());

        let drain = Filter::new(drain, move |r| level.accepts(r.level())).fuse();
        let drain = slog_async::Async::new(drain)
            .chan_size(4096)
            .overflow_strategy(OverflowStrategy::Block)
            .build()
            .fuse();
        let logger = Logger::root(drain, o!());

        slog_scope::set_global_logger(logger)
    }
}

#[cfg(test)]
pub mod test {
    use std::sync::Once;

    use slog::{Drain, Logger, o};

    static INIT_LOGGER: Once = Once::new();

    pub fn init_test_logger() {
        INIT_LOGGER.call_once(|| {
            let decorator = slog_term::PlainSyncDecorator::new(std::io::stdout());
            let drain = slog_term::FullFormat::new(decorator)
                .use_local_timestamp()
                .use_file_location()
                .build()
                .fuse();
            let logger = Logger::root(drain, o!());

            let guard = slog_scope::set_global_logger(logger);

            // bit of a hack to ensure the guard stay around "forever"
            std::mem::forget(guard);
        });
    }
}