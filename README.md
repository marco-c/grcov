# grcov

[![Build Status](https://travis-ci.org/mozilla/grcov.svg?branch=master)](https://travis-ci.org/mozilla/grcov)
[![Build status](https://ci.appveyor.com/api/projects/status/1957u00h26alxey2/branch/master?svg=true)](https://ci.appveyor.com/project/marco-c/grcov)
[![codecov](https://codecov.io/gh/mozilla/grcov/branch/master/graph/badge.svg)](https://codecov.io/gh/mozilla/grcov)

grcov collects and aggregates code coverage information for multiple source files.

This is a project initiated by Mozilla to gather code coverage results on Firefox.

## Usage

1. Download grcov from https://github.com/mozilla/grcov/releases or run ```cargo install grcov```
2. Run grcov:

```
Usage: grcov DIRECTORY[...] [-t OUTPUT_TYPE] [-s SOURCE_ROOT] [--token COVERALLS_REPO_TOKEN]
You can specify one or more directories, separated by a space.
OUTPUT_TYPE can be one of:
 - (DEFAULT) ade for the ActiveData-ETL specific format;
 - lcov for the lcov INFO format;
 - coveralls for the Coveralls specific format.
SOURCE_ROOT is the root directory of the source files, required for the 'coveralls' format.
REPO_TOKEN is the repository token from Coveralls, required for the 'coveralls' format.
```

Let's see a few examples, assuming the source directory is `~/Documents/mozilla-central` and the build directory is `~/Documents/mozilla-central/build`.

### LCOV output

```sh
grcov ~/Documents/mozilla-central/build -t lcov > lcov.info
```

As the LCOV output is compatible with `lcov`, `genhtml` can be used to generate a HTML summary of the code coverage:
```sh
genhtml -o report/ --show-details --highlight --ignore-errors source --legend lcov.info
```

### Coveralls/Codecov output

```sh
grcov ~/Documents/FD/mozilla-central/build -t coveralls -s ~/Documents/FD/mozilla-central --token YOUR_COVERALLS_TOKEN > coveralls.json
```

## Build & Test

```
cargo build
# By default, the binary is generated in ./target/debug/grcov
```

To test the binary:
```
cargo test
```

## Minimum requirements

- GCC 4.9 or higher is required.

## License

Published under the MPL 2.0 license
