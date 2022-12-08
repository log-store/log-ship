#!/bin/bash

set -e

INT_TESTS_DIR=$(pwd)

cp metrics_insert_ts_stdout.toml /tmp/test.toml

# run the build, just in case
cargo build --release

# run log-ship
../target/release/log-ship --config-file /tmp/test.toml

