# cargo-cacher

[![Build Status](https://travis-ci.org/ChrisMacNaughton/cargo-cacher.svg?branch=master)](https://travis-ci.org/ChrisMacNaughton/cargo-cacher)[![Coverage Status](https://coveralls.io/repos/github/ChrisMacNaughton/cargo-cacher/badge.svg?branch=master)](https://coveralls.io/github/ChrisMacNaughton/cargo-cacher?branch=master)

`cargo-cacher` is a caching server in the same spirit as apt-cacher-ng. The goal is to allow recursive caching of the canonical crates.io index.

## Usage

To configure your system to use a copy of cargo-cacher, you need to setup a .cargo/config file in your project, or in a containing folder. The contents of that folder should look like:

```toml
[source]

[source.mirror]
registry = "http://localhost:8080/index"

[source.crates-io]
replace-with = "mirror"
```

Once this is in place, your builds will go through the local proxy, and the crates will be pulled down to the local filesystem when they are first requested. The path can be a remote hosst as long as the path is to /index. To run cargo-cacher, there are several arguments you probably want to use:

```
USAGE:
    cargo-cacher [FLAGS] [OPTIONS]

FLAGS:
    -d               Sets the level of debugging information
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -g <git>             Upstream git index (Default: https://github.com/rust-lang/crates.io-index.git)
    -i <index>           Path to store the indexes (git and fiels) at (Default: ./index)
    -p <port>            Output file to put compiled crushmap into (Default: 8080)
    -f <prefetch>        Path with a list of crate_name=version to pre-fetch
    -r <refresh>         Refresh rate for the git index (Default: 600)
    -u <upstream>        Upstream Crate source (Default: https://crates.io/api/v1/crates/)

```

Prefetch is an option that I feel deserves further attention. Prefetch is a path to a file containing one line per crate/version, example:

```
log=0.3.6
libc=0.1.12
```

The above input will fetch log version 0.3.6 and libc version 0.1.12 before being requested by a user. This happens on a separate thread so the server can continue to start up without waiting on the pre-fetching to complete.

## TODO

- Add expiration on background thread
- Add statistics
