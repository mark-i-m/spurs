# spurs

[![Latest version](https://img.shields.io/crates/v/spurs.svg)](https://crates.io/crates/spurs)
[![Documentation](https://docs.rs/spurs/badge.svg)](https://docs.rs/spurs)

Utilities for setting up and running experiments remotely.

This is kind of a "living repo". I will add to it utilities as I find them
useful (or people submit PRs).

It is heavily inspired by [spur.py](https://github.com/mwilliamson/spur.py) but
it also contains a bunch of utility routines that I have found useful.

## Features

- A straight-forward, well-typed, idiomatic interface for running commands
  remotely via SSH, similar to `spur.py`.
- Utilities for common Linux admin operations, such as adding a user to a group
  or turning on a swap device or installing a package. This is by no means a
  complete set of such functions. So far it just includes the ones I have run
  into. Feel free to make PRs adding more.

## Example

```rust
use spurs::{cmd, ssh::SshShell};

let host = "1.2.3.4:22";
let shell = SshShell::with_default_key(username, host)?;

const FOO: &str = "foo";

shell.run(cmd!("ps -e | grep {}", FOO).use_bash())?;
shell.run(cmd!("ls -a | grep user").cwd("/home/user/").use_bash())?;
shell.run(spurs::centos::yum_install(&["qemu-kvm"]))?;
```

## Usage

Add the crate `spurs` to your `Cargo.toml` dependencies.

This crate uses Rust 2018, so it's unstable until December 2018, but at that
time, I believe it should become usable on stable rust.
