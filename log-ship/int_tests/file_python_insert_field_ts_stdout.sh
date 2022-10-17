#!/bin/bash

set -e

INT_TESTS_DIR=$(pwd)

# update the config file with the correct path
sed "s,INT_TEST_DIR,$INT_TESTS_DIR,g" < file_python_insert_field_ts_stdout.toml > /tmp/test.toml

# run the build, just in case
cargo build --release

# remove the old state file
rm test_input1.txt.state

# run log-ship
../../target/release/log-ship --config-file /tmp/test.toml

# remove state file
rm test_input1.txt.state

