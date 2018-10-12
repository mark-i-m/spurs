//! Functionality related to connecting, starting, maintaining, and executing commands over SSH.

use std::{
    io::Read,
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
};

use failure::Fail;

use ssh2::Session;

/// An error type representing things that could possibly go wrong when using an SshShell.
#[derive(Debug, Fail)]
pub enum SshError {
    #[fail(display = "no such key: {}", file)]
    KeyNotFound { file: String },

    #[fail(
        display = "authentication failed with private key: {:?}",
        key
    )]
    AuthFailed { key: PathBuf },

    #[fail(display = "Non-zero exit ({}) for command: {}", exit, cmd)]
    NonZeroExit { cmd: String, exit: i32 },
}

/// Represents a connection via SSH to a particular source.
pub struct SshShell {
    // The TCP stream needs to be in the struct to keep it alive while the session is active.
    _tcp: TcpStream,
    sess: Session,
}

impl SshShell {
    /// Returns a shell connected via the default private key at `$HOME/.ssh/id_rsa` to the given
    /// SSH server as the given user.
    pub fn with_default_key<A: ToSocketAddrs>(
        username: &str,
        remote: A,
    ) -> Result<Self, failure::Error> {
        const DEFAULT_KEY_SUFFIX: &str = ".ssh/id_rsa";
        let home = if let Some(home) = dirs::home_dir() {
            home
        } else {
            return Err(SshError::KeyNotFound {
                file: DEFAULT_KEY_SUFFIX.into(),
            }
            .into());
        };
        SshShell::with_key(username, remote, home.join(DEFAULT_KEY_SUFFIX))
    }

    /// Returns a shell connected via private key file `key` to the given SSH server as the given
    /// user.
    pub fn with_key<A: ToSocketAddrs, P: AsRef<Path>>(
        username: &str,
        remote: A,
        key: P,
    ) -> Result<Self, failure::Error> {
        let tcp = TcpStream::connect(remote)?;
        let mut sess = Session::new().unwrap();
        sess.handshake(&tcp)?;
        sess.userauth_pubkey_file(username, None, key.as_ref(), None)?;
        if !sess.authenticated() {
            return Err(SshError::AuthFailed {
                key: key.as_ref().to_path_buf(),
            }
            .into());
        }
        Ok(SshShell { _tcp: tcp, sess })
    }

    /// Run a command on the remote machine, blocking until the command completes.
    pub fn run(&self, cmd: &str) -> Result<(), failure::Error> {
        let mut chan = self.sess.channel_session()?;

        // print message
        println!("{}", console::style(cmd).yellow().bold());

        // execute cmd remotely
        chan.exec(cmd)?;

        // print stdout
        let mut buf = [0; 256];
        while chan.read(&mut buf)? > 0 {
            print!("{}", std::str::from_utf8(&buf).unwrap());
        }

        // close and wait for remote to close
        chan.close()?;
        chan.wait_close()?;

        // print stderr
        while chan.stderr().read(&mut buf)? > 0 {
            print!("{}", std::str::from_utf8(&buf).unwrap());
        }

        // check the exit status
        let exit = chan.exit_status()?;
        if exit != 0 {
            return Err(SshError::NonZeroExit {
                cmd: cmd.into(),
                exit,
            }
            .into());
        }
        Ok(())
    }

    // TODO: run with bash
    // TODO: spawn
}

#[test]
fn test_easy() {
    let ushell = SshShell::with_default_key("markm", "seclab8:22").unwrap();
    ushell.run("ls").unwrap();
}
