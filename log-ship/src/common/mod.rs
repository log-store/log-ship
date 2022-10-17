pub mod logging;
pub mod config_utils;


pub use logging::{setup, setup_with_level, FilterLevel, error, debug, info, warn};

#[cfg(test)]
pub use logging::test::init_test_logger;


#[macro_export]
macro_rules! duration_ms {
    ($exp:expr) => {{
        $exp.elapsed().as_millis()
    }};
}

#[macro_export]
macro_rules! duration_us {
    ($exp:expr) => {{
        $exp.elapsed().as_micros()
    }};
}

#[macro_export]
macro_rules! duration_ns {
    ($exp:expr) => {{
        $exp.elapsed().as_nanos()
    }};
}

#[macro_export]
macro_rules! duration_s {
    ($exp:expr) => {{
        $exp.elapsed().as_secs_f64()
    }};
}
