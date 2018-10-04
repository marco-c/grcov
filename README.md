# grcov

[![Build Status](https://travis-ci.org/mozilla/grcov.svg?branch=master)](https://travis-ci.org/mozilla/grcov)
[![Build status](https://ci.appveyor.com/api/projects/status/1957u00h26alxey2/branch/master?svg=true)](https://ci.appveyor.com/project/marco-c/grcov)
[![Coverage Status](https://coveralls.io/repos/github/mozilla/grcov/badge.svg)](https://coveralls.io/github/mozilla/grcov)

grcov collects and aggregates code coverage information for multiple source files.

## Usage

1. Download grcov from https://github.com/mozilla/grcov/releases.
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

Let's see a few examples, assuming the source directory is `~/Documenti/mozilla-central` and the build directory is `~/Documenti/mozilla-central/build`.

### LCOV output

```sh
grcov ~/Documenti/mozilla-central/build -t lcov > lcov.info
```

As the LCOV output is compatible with `lcov`, `genhtml` can be used to generate a HTML summary of the code coverage:
```sh
genhtml -o report/ --show-details --highlight --ignore-errors source --legend lcov.info
```

### Coveralls/Codecov output

```sh
grcov ~/Documenti/FD/mozilla-central/build -t coveralls -s ~/Documenti/FD/mozilla-central --token YOUR_COVERALLS_TOKEN > coveralls.json
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
