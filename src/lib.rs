//! This is a library containing a bunch of routines that I have found useful for setting up and
//! running experiments remotely.

#![doc(html_root_url = "https://docs.rs/spurs/0.4.2")]

#[macro_use]
pub mod ssh;

pub mod centos;
pub mod ubuntu;
pub mod util;
