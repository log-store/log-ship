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