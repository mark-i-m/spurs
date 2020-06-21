# Changelog

## 0.9.0
- Major changes to the way `spawn` and `SshSpawnHandle` work, along with their
  APIs.
  - Removed `spawn` from `Execute` and added `duplicate` trait method instead.
  - Implemented `spawn` directly on `SshShell` and made it `duplicate` an
    existing handle. This avoids buggy synchronization and output issues.
  - `spawn` no longer returns the spawned `SshShell` until the command
    terminates, in which case it will be returned by `join`ing `SshSpawnHandle`.
    This also simplifies usage of the spawn handle.
  - The simplifications here reduce the bugginess of `spawn` and bring it in
    line with what I've been doing manually anyway lately: creating a new SSH
    shell and running things on it in parallel.

## 0.8.0, 0.8.1, 0.8.2
- Improve output format.
- Minor changes, dependency updates, and documentation updates.

## 0.7.0
- Move all utilities to the `spurs-util` crate, making `spurs` purely about
  minimal SSH functionality.
- Minor bug fix.

## 0.6.0
- Move to custom error type, rather than failure.

## 0.5.0

- Reimplemented spawn to allow true parallelism. It now opens a new SSH
  connection to the remote for the command. The `Execute` trait has been
  updated accordingly to return the new shell.
- All except for the most fundamental utilities have been moved to the
  new `spurs-util` crate.
- The (textual) hostname is preserved when printing messages.
- When a command is executed, the username and hostname are printed too, making
  it easier to see what is running on which machine as which user.
- Updated dependencies.
- Started this changelog :)
