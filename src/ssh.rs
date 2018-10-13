//! Functionality related to connecting, starting, maintaining, and executing commands over SSH.

use std::{
    io::Read,
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::JoinHandle,
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

    #[fail(display = "non-zero exit ({}) for command: {}", exit, cmd)]
    NonZeroExit { cmd: String, exit: i32 },
}

pub struct SshCommand {
    cmd: String,
    cwd: Option<PathBuf>,
    use_bash: bool,
}

/// Represents a connection via SSH to a particular source.
pub struct SshShell {
    // The TCP stream needs to be in the struct to keep it alive while the session is active.
    _tcp: TcpStream,
    sess: Arc<Mutex<Session>>,
}

/// A handle for a spawned remote command.
pub struct SshSpawnHandle {
    thread_handle: JoinHandle<Result<(), failure::Error>>,
}

impl SshCommand {
    pub fn new(cmd: &str) -> Self {
        SshCommand {
            cmd: cmd.to_owned(),
            cwd: None,
            use_bash: false,
        }
    }

    pub fn cwd<P: AsRef<Path>>(self, cwd: P) -> Self {
        SshCommand {
            cwd: Some(cwd.as_ref().to_owned()),
            ..self
        }
    }

    pub fn use_bash(self) -> Self {
        SshCommand {
            use_bash: true,
            ..self
        }
    }
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
        Ok(SshShell {
            _tcp: tcp,
            sess: Arc::new(Mutex::new(sess)),
        })
    }

    fn run_with_chan_and_opts(
        mut chan: ssh2::Channel,
        cmd_opts: SshCommand,
    ) -> Result<(), failure::Error> {
        let SshCommand { cwd, cmd, use_bash } = cmd_opts;

        // Print the raw command. We are going to modify it slightly before executing (e.g. to
        // switch directories)
        let msg = cmd.clone();

        // Construct the commmand in the right directory and using bash if needed.
        let cmd = if use_bash {
            format!("bash -c {}", super::util::escape_for_bash(&cmd))
        } else {
            cmd
        };

        let cmd = if let Some(cwd) = cwd {
            format!("cd {} ; {}", cwd.display(), cmd)
        } else {
            cmd
        };

        // print message
        println!("{}", console::style(msg).yellow().bold());

        // execute cmd remotely
        chan.exec(&cmd)?;

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

    /// Run a command on the remote machine, blocking until the command completes.
    pub fn run(&self, cmd: SshCommand) -> Result<(), failure::Error> {
        let sess = self.sess.lock().unwrap();
        let chan = sess.channel_session()?;
        Self::run_with_chan_and_opts(chan, cmd)
    }

    /// Run a command on the remote machine, without blocking until the command completes. A handle
    /// is returned, which one can `join` on to wait for process completion.
    pub fn spawn(&self, cmd: SshCommand) -> Result<SshSpawnHandle, failure::Error> {
        let sess = self.sess.clone();
        Ok(SshSpawnHandle {
            thread_handle: std::thread::spawn(move || {
                let sess = sess.lock().unwrap();
                let chan = sess.channel_session()?;
                Self::run_with_chan_and_opts(chan, cmd)
            }),
        })
    }
}

impl SshSpawnHandle {
    /// Block until the remote command completes.
    pub fn join(self) -> Result<(), failure::Error> {
        self.thread_handle.join().unwrap()
    }
}
