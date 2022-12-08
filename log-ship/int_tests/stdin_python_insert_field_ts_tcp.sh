#!/bin/bash

set -e

INT_TESTS_DIR=$(pwd)

# update the config file with the correct path
sed "s,INT_TEST_DIR,$INT_TESTS_DIR,g" < file_python_insert_field_ts_stdout.toml > /tmp/test.toml

# run the build, just in case
cargo build --release

# remove the old state file
rm test_input1.txt.state || true

# run nc in the background to listen for the logs
nc -l 1234 &

# run log-ship
../target/release/log-ship --config-file /tmp/test.toml < test_input1.txt

# cleanup nc when we're done
kill $(ps aux | grep 1234 | awk '{ print $2 }' | head -n1)

