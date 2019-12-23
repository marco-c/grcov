# grcov

[![Build Status](https://travis-ci.org/mozilla/grcov.svg?branch=master)](https://travis-ci.org/mozilla/grcov)
[![Build status](https://ci.appveyor.com/api/projects/status/1957u00h26alxey2/branch/master?svg=true)](https://ci.appveyor.com/project/marco-c/grcov)
[![codecov](https://codecov.io/gh/mozilla/grcov/branch/master/graph/badge.svg)](https://codecov.io/gh/mozilla/grcov)
![crates.io](https://img.shields.io/crates/v/grcov.svg)

grcov collects and aggregates code coverage information for multiple source files.

This is a project initiated by Mozilla to gather code coverage results on Firefox.

## Table of Contents
* [Usage](#usage)
    * [LCOV output](#lcov-output)
    * [Coveralls/Codecov output](#coverallscodecov-output)
    * [grcov with Travis](#grcov-with-travis)
    * [Auto Formatting](#auto-formatting)
* [Build & Test](#build--test)
* [Minimum requirements](#minimum-requirements)
* [License](#license)

## Usage

1. Download grcov from https://github.com/mozilla/grcov/releases or run ```cargo install grcov```
2. Run grcov:

```
USAGE:
    grcov [FLAGS] [OPTIONS] <paths>...

FLAGS:
        --branch                          Enables parsing branch coverage information
        --guess-directory-when-missing
    -h, --help                            Prints help information
        --ignore-not-existing             Ignore source files that can't be found on the disk
        --llvm                            Speeds-up parsing, when the code coverage information is exclusively coming
                                          from a llvm build
    -V, --version                         Prints version information

OPTIONS:
        --commit-sha <COMMIT HASH>                   Sets the hash of the commit used to generate the code coverage data
        --filter <filter>
            Filters out covered/uncovered files. Use 'covered' to only return covered files, 'uncovered' to only return
            uncovered files [possible values: covered, uncovered]
        --ignore <PATH>...                           Ignore files/directories specified as globs
        --log <LOG>
            Set the file where to log (or stderr or stdout). Defaults to 'stderr' [default: stderr]

    -o, --output-file <FILE>                         Specifies the output file
    -t, --output-type <OUTPUT TYPE>
            Sets a custom output type [default: lcov]  [possible values: ade, lcov, coveralls, coveralls+, files,
            covdir, html]
        --path-mapping <PATH>...
    -p, --prefix-dir <PATH>
            Specifies a prefix to remove from the paths (e.g. if grcov is run on a different machine than the one that
            generated the code coverage information)
        --service-job-number <SERVICE JOB NUMBER>    Sets the service job number
        --service-name <SERVICE NAME>                Sets the service name
        --service-number <SERVICE NUMBER>            Sets the service number
        --service-pull-request <SERVICE PULL REQUEST>
                                                     Sets the service pull request number

    -s, --source-dir <DIRECTORY>                     Specifies the root directory of the source files
        --threads <NUMBER>                            [default: 16]
        --token <TOKEN>
            Sets the repository token from Coveralls, required for the 'coveralls' and 'coveralls+' formats

        --vcs-branch <VCS BRANCH>
            Set the branch for coveralls report. Defaults to 'master' [default: master]


ARGS:
    <paths>...    Sets the input paths to use
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

### grcov with Travis

Here is an example of .travis.yml file
```YAML
language: rust

before_install:
  - curl -L https://github.com/mozilla/grcov/releases/latest/download/grcov-linux-x86_64.tar.bz2 | tar jxf -

matrix:
  include:
    - os: linux
      rust: nightly

script:
    - export CARGO_INCREMENTAL=0
    - export RUSTFLAGS="-Zprofile -Ccodegen-units=1 -Cinline-threshold=0 -Clink-dead-code -Coverflow-checks=off -Zno-landing-pads"
    - cargo build --verbose $CARGO_OPTIONS
    - cargo test --verbose $CARGO_OPTIONS
    - |
      zip -0 ccov.zip `find . \( -name "YOUR_PROJECT_NAME*.gc*" \) -print`;
      ./grcov ccov.zip -s . -t lcov --llvm --branch --ignore-not-existing --ignore "/*" -o lcov.info;
      bash <(curl -s https://codecov.io/bash) -f lcov.info;
```

### Auto-formatting

This project is using pre-commit. Please run `pre-commit install` to install the git pre-commit hooks on your clone. Instructions on how to install pre-commit can be found [here](https://pre-commit.com/#install).

Every time you will try to commit, pre-commit will run checks on your files to make sure they follow our style standards and they aren't affected by some simple issues. If the checks fail, pre-commit won't let you commit.

## Build & Test

Build with:
```
cargo build
```

To run unit tests:
```
cargo test --lib
```

To run integration tests, it is suggested to use the Docker image defined in tests/Dockerfile. Simply build the image to run them:
```
docker build -t marcocas/grcov -f tests/Dockerfile .
```

Otherwise, if you don't want to use Docker, the only prerequisite is to install GCC 7, setting the `GCC_CXX` environment variable to `g++-7` and the `GCOV` environment variable to `gcov-7`. Then run the tests with:
```
cargo test
```

## Minimum requirements

- GCC 4.9 or higher is required (if parsing coverage artifacts generated by GCC).

## License

Published under the MPL 2.0 license.
