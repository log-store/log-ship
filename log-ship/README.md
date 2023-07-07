# Developing log-ship!

The systemd library is required to build log-ship. On a Debian/Ubuntu machine, this is the `libsystemd-dev` package.

# Building log-ship

* Run clippy to ensure things are reasonably clean
```shell
cargo clippy --fix && cargo clippy -- 
-A dead-code \
-A clippy::needless-return \
-A clippy::comparison-chain \
-A clippy::type-complexity \
-A clippy::new-without-default \
-A clippy::int-plus-one \
  --deny warnings
```
* Set the version in `Cargo.toml`
* Tag the release version `git tag -a v1.x.y && git push origin --tags`
  * You can use `git tag -a -f <tag_identifier> <commit_id>` to move a tag, but you must force-push to origin if you've already pushed
* Create the tarball:
  * `cargo build --release && strip target/release/log-ship`
  * `mkdir /tmp/build && cp target/release/log-ship /tmp/build/log-ship_1.x.y && cp ../cargo_deb/assets/scripts /tmp/build -R && cp ../cargo_deb/assets/log-ship.toml`
  * `cd /tmp/build && tar -zcvf log-ship_1.x.y.tar.gz scripts log-ship.toml log-ship_1.x.y`
* Create the packages:
  * `cargo deb`
  * `cargo generate-rpm`
* Update the site with the correct versions, and deploy
* Upload the site and artifacts
  * `scp target/release/log-ship_1.2.0.gz log-store:/var/www/log-ship`
  * `scp target/debian/log-ship_1.2.0_amd64.deb log-store:/var/www/log-ship`
  * `scp target/generate-rpm/log-ship-1.2.0-1.x86_64.rpm log-store:/var/www/log-ship`


# Static Building
**THIS WILL NOT BUILD A STATIC BINARY** journalctl library prevents it
The below 2 steps should generate a static binary:
* `./pyoxidizer generate-python-embedding-artifacts --target-triple x86_64-unknown-linux-musl --flavor standalone_static pyembedded`
* `PYO3_CONFIG_FILE=$(pwd)/pyembedded/pyo3-build-config-file.txt cargo build --target x86_64-unknown-linux-musl`

This will "embed" Python into the binary, but still be dynamic:
* `./pyoxidizer generate-python-embedding-artifacts pyembedded`
* `PYO3_CONFIG_FILE=$(pwd)/pyembedded/pyo3-build-config-file.txt cargo build`

```
PyOxidizer 0.24.0
commit: 42513bf0417cf61bf879be7360d55ae3d3bf2637
source: https://github.com/indygreg/PyOxidizer.git
pyembed crate location: version = "0.24.0"
```

* Needed to install libbsd: `sudo aptitude install libbsd-dev`
* Followed [pyoxidizer directions](https://pyoxidizer.readthedocs.io/en/latest/pyoxidizer_rust_generic_embedding.html#embed-python-with-pyo3)