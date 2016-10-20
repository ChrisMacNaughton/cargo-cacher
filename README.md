# cargo-cacher

[![Build Status](https://travis-ci.org/ChrisMacNaughton/cargo-cacher.svg?branch=master)](https://travis-ci.org/ChrisMacNaughton/cargo-cacher)[![Coverage Status](https://coveralls.io/repos/github/ChrisMacNaughton/cargo-cacher/badge.svg?branch=master)](https://coveralls.io/github/ChrisMacNaughton/cargo-cacher?branch=master)

`cargo-cacher` is a caching server in the same spirit as apt-cacher-ng. The goal is to allow recursive caching of the canonical crates.io index.

## Usage

To configure your system to use a copy of cargo-cacher, you need to setup a .cargo/config file in your project, or in a containing folder. The contents of that folder should look like:

```toml
[source]

[source.mirror]
registry = "file:///configured/path/to/index"

[source.crates-io]
replace-with = "mirror"
registry = "https://doesnt-matter-but-must-be-present"
```

Once this is in place, your builds will go through the local proxy, and the crates will be pulled down to the local filesystem when they are first requested.

## TODO

- Add expiration on background thread
- Add statistics
- Remote git server