# Changelog

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
