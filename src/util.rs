//! A collection of useful utilities for running commands, configuring machines, etc.

/// Given a string, encode all single quotes so that the whole string can be passed correctly as a
/// single argument to a bash command.
///
/// This is useful for passing commands to `bash -c` (e.g. through ssh).
///
/// For example, if I want to run the following command:
///
/// ```bash
/// echo '$HELLOWORLD="hello world"' | grep "hello"
/// ```
///
/// This function will output `'echo '"'"'$HELLOWORLD="hello world"'"'"' | grep "hello"'`.
/// So the following command can be executed over ssh:
///
/// ```bash
/// bash -c 'echo '"'"'$HELLOWORLD="hello world"'"'"' | grep "hello"'
/// ```
pub fn escape_for_bash(s: &str) -> String {
    let mut new = String::with_capacity(s.len());

    new.push('\'');

    for c in s.chars() {
        if c == '\'' {
            new.push('\''); // end first part of string

            new.push('"');
            new.push('\''); // quote the single quote
            new.push('"');

            new.push('\''); // start next part of string
        } else {
            new.push(c);
        }
    }

    new.push('\'');

    new
}
