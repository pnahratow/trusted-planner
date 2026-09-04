Open source attribution
=======================

Trusted Planner is AGPL-3.0-or-later (see LICENSE). It stands on other
people's work, and a released binary carries that work inside it: SQLite is
compiled into it, the timezone database with it, and every crate below is
linked in. The two JavaScript libraries are served as files rather than
compiled, and are covered by static/vendor/NOTICE.

This list is generated from Cargo.lock — regenerate it after changing a
dependency:

    cargo metadata --format-version 1 --locked

The licence texts are the standard ones named here, and each crate ships its
own copyright notice in its source, which cargo keeps under ~/.cargo/registry
and which the crates.io page for each name reproduces.


Rust crates linked into the binary (136)
----------------------------------------

  aho-corasick                 1.1.5                          Unlicense OR MIT
  android_system_properties    0.1.6                          MIT OR Apache-2.0
  anyhow                       1.0.104                        MIT OR Apache-2.0
  atomic-waker                 1.1.2                          Apache-2.0 OR MIT
  autocfg                      1.5.1                          Apache-2.0 OR MIT
  axum                         0.8.9                          MIT
  axum-core                    0.5.6                          MIT
  axum-extra                   0.12.6                         MIT
  bitflags                     2.13.1                         MIT OR Apache-2.0
  bumpalo                      3.20.3                         MIT OR Apache-2.0
  bytes                        1.12.1                         MIT
  cc                           1.4.4                          MIT OR Apache-2.0
  cfg-if                       1.0.4                          MIT OR Apache-2.0
  chrono                       0.4.45                         MIT OR Apache-2.0
  chrono-tz                    0.10.4                         MIT OR Apache-2.0
  cookie                       0.18.2                         MIT OR Apache-2.0
  core-foundation-sys          0.8.7                          MIT OR Apache-2.0
  deranged                     0.5.8                          MIT OR Apache-2.0
  equivalent                   1.0.2                          Apache-2.0 OR MIT
  errno                        0.3.14                         MIT OR Apache-2.0
  fallible-iterator            0.3.0                          MIT/Apache-2.0
  fallible-streaming-iterator  0.1.9                          MIT/Apache-2.0
  find-msvc-tools              0.1.11                         MIT OR Apache-2.0
  foldhash                     0.2.0                          Zlib
  form_urlencoded              1.2.2                          MIT OR Apache-2.0
  futures-channel              0.3.34                         MIT OR Apache-2.0
  futures-core                 0.3.34                         MIT OR Apache-2.0
  futures-sink                 0.3.34                         MIT OR Apache-2.0
  futures-task                 0.3.34                         MIT OR Apache-2.0
  futures-util                 0.3.34                         MIT OR Apache-2.0
  hashbrown                    0.16.1                         MIT OR Apache-2.0
  hashbrown                    0.17.1                         MIT OR Apache-2.0
  hashlink                     0.12.1                         MIT OR Apache-2.0
  http                         1.5.0                          MIT OR Apache-2.0
  http-body                    1.1.0                          MIT
  http-body-util               0.1.5                          MIT
  http-range-header            0.4.2                          MIT
  httparse                     1.10.1                         MIT OR Apache-2.0
  httpdate                     1.0.3                          MIT OR Apache-2.0
  hyper                        1.11.1                         MIT
  hyper-util                   0.1.20                         MIT
  iana-time-zone               0.1.65                         MIT OR Apache-2.0
  iana-time-zone-haiku         0.1.2                          MIT OR Apache-2.0
  indexmap                     2.14.1                         Apache-2.0 OR MIT
  itoa                         1.0.18                         MIT OR Apache-2.0
  js-sys                       0.3.104                        MIT OR Apache-2.0
  lazy_static                  1.5.0                          MIT OR Apache-2.0
  libc                         0.2.189                        MIT OR Apache-2.0
  libsqlite3-sys               0.38.2                         MIT
  lock_api                     0.4.14                         MIT OR Apache-2.0
  log                          0.4.34                         MIT OR Apache-2.0
  matchers                     0.2.0                          MIT
  matchit                      0.8.4                          MIT AND BSD-3-Clause
  memchr                       2.8.3                          Unlicense OR MIT
  memo-map                     0.3.4                          Apache-2.0
  mime                         0.3.17                         MIT OR Apache-2.0
  mime_guess                   2.0.5                          MIT
  minijinja                    2.24.0                         Apache-2.0
  mio                          1.2.3                          MIT
  nu-ansi-term                 0.50.3                         MIT
  num-conv                     0.2.2                          MIT OR Apache-2.0
  num-traits                   0.2.19                         MIT OR Apache-2.0
  once_cell                    1.21.4                         MIT OR Apache-2.0
  parking_lot                  0.12.5                         MIT OR Apache-2.0
  parking_lot_core             0.9.12                         MIT OR Apache-2.0
  percent-encoding             2.3.2                          MIT OR Apache-2.0
  phf                          0.12.1                         MIT
  phf_shared                   0.12.1                         MIT
  pin-project-lite             0.2.17                         Apache-2.0 OR MIT
  pkg-config                   0.3.34                         MIT OR Apache-2.0
  powerfmt                     0.2.0                          MIT OR Apache-2.0
  proc-macro2                  1.0.107                        MIT OR Apache-2.0
  quote                        1.0.47                         MIT OR Apache-2.0
  redox_syscall                0.5.18                         MIT
  regex-automata               0.4.18                         MIT OR Apache-2.0
  regex-syntax                 0.8.11                         MIT OR Apache-2.0
  rsqlite-vfs                  0.1.1                          MIT
  rusqlite                     0.40.2                         MIT
  rustversion                  1.0.23                         MIT OR Apache-2.0
  ryu                          1.0.23                         Apache-2.0 OR BSL-1.0
  scopeguard                   1.2.0                          MIT OR Apache-2.0
  serde                        1.0.229                        MIT OR Apache-2.0
  serde_core                   1.0.229                        MIT OR Apache-2.0
  serde_derive                 1.0.229                        MIT OR Apache-2.0
  serde_html_form              0.2.8                          MIT
  serde_json                   1.0.151                        MIT OR Apache-2.0
  serde_path_to_error          0.1.20                         MIT OR Apache-2.0
  serde_urlencoded             0.7.1                          MIT/Apache-2.0
  sharded-slab                 0.1.7                          MIT
  shlex                        2.0.1                          MIT OR Apache-2.0
  signal-hook-registry         1.4.8                          MIT OR Apache-2.0
  siphasher                    1.0.3                          MIT/Apache-2.0
  slab                         0.4.12                         MIT
  smallvec                     1.16.0                         MIT OR Apache-2.0
  socket2                      0.6.5                          MIT OR Apache-2.0
  sqlite-wasm-rs               0.5.5                          MIT
  syn                          2.0.119                        MIT OR Apache-2.0
  syn                          3.0.4                          MIT OR Apache-2.0
  sync_wrapper                 1.0.2                          Apache-2.0
  thiserror                    2.0.20                         MIT OR Apache-2.0
  thiserror-impl               2.0.20                         MIT OR Apache-2.0
  thread_local                 1.1.10                         MIT OR Apache-2.0
  time                         0.3.55                         MIT OR Apache-2.0
  time-core                    0.1.9                          MIT OR Apache-2.0
  time-macros                  0.2.32                         MIT OR Apache-2.0
  tokio                        1.53.1                         MIT
  tokio-macros                 2.7.2                          MIT
  tokio-stream                 0.1.19                         MIT
  tokio-util                   0.7.19                         MIT
  tower                        0.5.3                          MIT
  tower-http                   0.7.1                          MIT
  tower-layer                  0.3.3                          MIT
  tower-service                0.3.3                          MIT
  tracing                      0.1.44                         MIT
  tracing-attributes           0.1.31                         MIT
  tracing-core                 0.1.36                         MIT
  tracing-log                  0.2.0                          MIT
  tracing-subscriber           0.3.23                         MIT
  unicase                      2.9.0                          MIT OR Apache-2.0
  unicode-ident                1.0.24                         (MIT OR Apache-2.0) AND Unicode-3.0
  valuable                     0.1.1                          MIT
  vcpkg                        0.2.15                         MIT/Apache-2.0
  version_check                0.9.5                          MIT/Apache-2.0
  wasi                         0.11.1+wasi-snapshot-preview1  Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT
  wasm-bindgen                 0.2.127                        MIT OR Apache-2.0
  wasm-bindgen-macro           0.2.127                        MIT OR Apache-2.0
  wasm-bindgen-macro-support   0.2.127                        MIT OR Apache-2.0
  wasm-bindgen-shared          0.2.127                        MIT OR Apache-2.0
  windows-core                 0.62.2                         MIT OR Apache-2.0
  windows-implement            0.60.2                         MIT OR Apache-2.0
  windows-interface            0.59.3                         MIT OR Apache-2.0
  windows-link                 0.2.1                          MIT OR Apache-2.0
  windows-result               0.4.1                          MIT OR Apache-2.0
  windows-strings              0.5.1                          MIT OR Apache-2.0
  windows-sys                  0.61.2                         MIT OR Apache-2.0
  zmij                         1.0.23                         MIT


Bundled C code
--------------

  SQLite, compiled in by rusqlite's `bundled` feature, is in the public
  domain: https://sqlite.org/copyright.html

