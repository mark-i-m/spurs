//! Functionality related to connecting, starting, maintaining, and executing commands over SSH.

use std::{
    io::Read,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use failure::Fail;

use log::{debug, info, trace};

use ssh2::Session;

/// The default timeout for the TCP stream of a SSH connection.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// An error type representing things that could possibly go wrong when using an SshShell.
#[derive(Debug, Fail)]
pub enum SshError {
    #[fail(display = "no such key: {}", file)]
    KeyNotFound { file: String },

    #[fail(display = "authentication failed with private key: {:?}", key)]
    AuthFailed { key: PathBuf },

    #[fail(display = "non-zero exit ({}) for command: {}", exit, cmd)]
    NonZeroExit { cmd: String, exit: i32 },
}

#[derive(Debug)]
pub struct SshCommand {
    cmd: String,
    cwd: Option<PathBuf>,
    use_bash: bool,
    allow_error: bool,
    dry_run: bool,
    no_pty: bool,
}

#[derive(Debug)]
pub struct SshOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Represents a connection via SSH to a particular source.
pub struct SshShell {
    // The TCP stream needs to be in the struct to keep it alive while the session is active.
    tcp: TcpStream,
    username: String,
    key: PathBuf,
    remote: SocketAddr,
    sess: Arc<Mutex<Session>>,
    dry_run_mode: bool,
}

/// A handle for a spawned remote command.
pub struct SshSpawnHandle {
    thread_handle: JoinHandle<Result<SshOutput, failure::Error>>,
}

impl SshCommand {
    /// Create a new builder for the given command with default options.
    pub fn new(cmd: &str) -> Self {
        SshCommand {
            cmd: cmd.to_owned(),
            cwd: None,
            use_bash: false,
            allow_error: false,
            dry_run: false,
            no_pty: false,
        }
    }

    /// Change the current working directory to `cwd` before executing.
    pub fn cwd<P: AsRef<Path>>(self, cwd: P) -> Self {
        SshCommand {
            cwd: Some(cwd.as_ref().to_owned()),
            ..self
        }
    }

    /// Execute using bash.
    pub fn use_bash(self) -> Self {
        SshCommand {
            use_bash: true,
            ..self
        }
    }

    /// Allow a non-zero exit code. Normally, an error would occur and we would return early.
    pub fn allow_error(self) -> Self {
        SshCommand {
            allow_error: true,
            ..self
        }
    }

    /// Don't actually execute any command remotely. Just print the command that would be executed
    /// and return success. Note that we still connect to the remote. This is useful for debugging.
    pub fn dry_run(self, is_dry: bool) -> Self {
        SshCommand {
            dry_run: is_dry,
            ..self
        }
    }

    /// Don't request a psuedo-terminal (pty). It turns out that some commands behave differently
    /// with a pty. I'm not really sure what causes this.
    ///
    /// NOTE: You need a pty for `sudo`.
    pub fn no_pty(self) -> Self {
        SshCommand {
            no_pty: true,
            ..self
        }
    }
}

impl SshShell {
    /// Returns a shell connected via the default private key at `$HOME/.ssh/id_rsa` to the given
    /// SSH server as the given user.
    ///
    /// ```rust,ignore
    /// SshShell::with_default_key("markm", "myhost:22")?;
    /// ```
    pub fn with_default_key<A: ToSocketAddrs + std::fmt::Debug>(
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
    ///
    /// ```rust,ignore
    /// SshShell::with_key("markm", "myhost:22", "/home/foo/.ssh/id_rsa")?;
    /// ```
    pub fn with_key<A: ToSocketAddrs + std::fmt::Debug, P: AsRef<Path>>(
        username: &str,
        remote: A,
        key: P,
    ) -> Result<Self, failure::Error> {
        info!("New SSH shell: {}@{:?}", username, remote);
        debug!("Using key: {:?}", key.as_ref());

        debug!("Create new TCP stream...");

        // Create a TCP connection
        let tcp = TcpStream::connect(&remote)?;
        tcp.set_read_timeout(Some(DEFAULT_TIMEOUT))?;
        tcp.set_write_timeout(Some(DEFAULT_TIMEOUT))?;
        let remote = remote.to_socket_addrs().unwrap().next().unwrap();

        debug!("Create new SSH session...");

        // Start an SSH session
        let mut sess = Session::new().unwrap();
        sess.handshake(&tcp)?;
        trace!("SSH session handshook.");
        sess.userauth_pubkey_file(username, None, key.as_ref(), None)?;
        if !sess.authenticated() {
            return Err(SshError::AuthFailed {
                key: key.as_ref().to_path_buf(),
            }
            .into());
        }
        trace!("SSH session authenticated.");

        println!(
            "{}",
            console::style(format!("{}@{}", username, remote))
                .green()
                .bold()
        );

        Ok(SshShell {
            tcp,
            username: username.to_owned(),
            key: key.as_ref().to_owned(),
            remote,
            sess: Arc::new(Mutex::new(sess)),
            dry_run_mode: false,
        })
    }

    /// Toggles _dry run mode_. In dry run mode, commands are not executed remotely; we only print
    /// what commands we would execute. Note that we do connect remotely, though. This is off by
    /// default: we default to actually running the commands.
    pub fn toggle_dry_run(&mut self) {
        self.dry_run_mode = !self.dry_run_mode;
        info!(
            "Toggled dry run mode: {}",
            if self.dry_run_mode { "on" } else { "off" }
        );
    }

    /// Attempt to reconnect to the remote until it reconnects (possibly indefinitely).
    pub fn reconnect(&mut self) -> Result<(), failure::Error> {
        info!("Reconnect attempt.");

        trace!("Attempt to create new TCP stream...");
        loop {
            print!("{}", console::style("Attempt Reconnect ... ").red());
            match TcpStream::connect_timeout(&self.remote, DEFAULT_TIMEOUT / 2) {
                Ok(tcp) => {
                    self.tcp = tcp;
                    break;
                }
                Err(e) => {
                    trace!("{:?}", e);
                    println!("{}", console::style("failed, retrying").red());
                    std::thread::sleep(DEFAULT_TIMEOUT / 2);
                }
            }
        }

        println!(
            "{}",
            console::style("TCP connected, doing SSH handshake").red()
        );

        // Start an SSH session
        debug!("Attempt to create new SSH session...");
        let mut sess = Session::new().unwrap();
        sess.handshake(&self.tcp)?;
        trace!("Handshook!");
        sess.userauth_pubkey_file(&self.username, None, self.key.as_ref(), None)?;
        if !sess.authenticated() {
            return Err(SshError::AuthFailed {
                key: self.key.clone(),
            }
            .into());
        }
        trace!("authenticated!");

        self.sess = Arc::new(Mutex::new(sess));

        println!(
            "{}",
            console::style(format!("{}@{}", self.username, self.remote))
                .green()
                .bold()
        );

        Ok(())
    }

    fn run_with_chan_and_opts(
        mut chan: ssh2::Channel,
        cmd_opts: SshCommand,
    ) -> Result<SshOutput, failure::Error> {
        debug!("run_with_chan_and_opts({:?})", cmd_opts);

        let SshCommand {
            cwd,
            cmd,
            use_bash,
            allow_error,
            dry_run,
            no_pty,
        } = cmd_opts;

        // Print the raw command. We are going to modify it slightly before executing (e.g. to
        // switch directories)
        let msg = cmd.clone();

        // Construct the commmand in the right directory and using bash if needed.
        let cmd = if use_bash {
            format!("bash -c {}", crate::util::escape_for_bash(&cmd))
        } else {
            cmd
        };

        debug!("After shell escaping: {:?}", cmd);

        let cmd = if let Some(cwd) = cwd {
            format!("cd {} ; {}", cwd.display(), cmd)
        } else {
            cmd
        };

        debug!("After cwd: {:?}", cmd);

        // print message
        println!("{}", console::style(msg).yellow().bold());

        let mut stdout = String::new();
        let mut stderr = String::new();

        // If dry run, close and return early without actually doing anything.
        if dry_run {
            chan.close()?;
            chan.wait_close()?;

            debug!("Closed channel after dry run.");

            return Ok(SshOutput { stdout, stderr });
        }

        // request a pty so that `sudo` commands work fine
        if !no_pty {
            chan.request_pty("vt100", None, None)?;
            debug!("Requested pty.");
        }

        // execute cmd remotely
        debug!("Execute command remotely (asynchronous)...");
        chan.exec(&cmd)?;

        trace!("Read stdout...");

        // print stdout
        let mut buf = [0; 256];
        while chan.read(&mut buf)? > 0 {
            let out = std::str::from_utf8(&buf)
                .unwrap()
                .trim_right_matches('\u{0}');
            print!("{}", out);
            stdout.push_str(out);

            // clear buf
            buf.iter_mut().for_each(|x| *x = 0);
        }

        trace!("No more stdout.");

        // close and wait for remote to close
        chan.close()?;
        chan.wait_close()?;

        debug!("Command completed remotely.");

        // clear buf
        buf.iter_mut().for_each(|x| *x = 0);

        trace!("Read stderr...");

        // print stderr
        while chan.stderr().read(&mut buf)? > 0 {
            let err = std::str::from_utf8(&buf)
                .unwrap()
                .trim_right_matches('\u{0}');
            print!("{}", err);
            stderr.push_str(err);

            // clear buf
            buf.iter_mut().for_each(|x| *x = 0);
        }

        trace!("No more stderr.");
        debug!("Checking exit status.");

        // check the exit status
        let exit = chan.exit_status()?;
        debug!("Exit status: {}", exit);
        if exit != 0 && !allow_error {
            return Err(SshError::NonZeroExit { cmd, exit }.into());
        }

        trace!("Done with command.");

        // return output
        Ok(SshOutput { stdout, stderr })
    }

    /// Run a command on the remote machine, blocking until the command completes.
    ///
    /// Note that command using `sudo` will hang indefinitely if `sudo` asks for a password.
    pub fn run(&self, cmd: SshCommand) -> Result<SshOutput, failure::Error> {
        debug!("run(cmd)");
        let sess = self.sess.lock().unwrap();
        debug!("Attempt to crate channel...");
        let chan = sess.channel_session()?;
        let cmd = if self.dry_run_mode {
            cmd.dry_run(true)
        } else {
            cmd
        };
        Self::run_with_chan_and_opts(chan, cmd)
    }

    /// Run a command on the remote machine, without blocking until the command completes. A handle
    /// is returned, which one can `join` on to wait for process completion.
    ///
    /// Note that command using `sudo` will hang indefinitely if `sudo` asks for a password.
    pub fn spawn(&self, cmd: SshCommand) -> Result<SshSpawnHandle, failure::Error> {
        debug!("spawn(cmd)");
        let sess = self.sess.clone();
        let cmd = if self.dry_run_mode {
            cmd.dry_run(true)
        } else {
            cmd
        };

        let thread_handle = std::thread::spawn(move || {
            let sess = sess.lock().unwrap();
            debug!("Attempt to crate channel for spawned command...");
            let chan = sess.channel_session()?;
            Self::run_with_chan_and_opts(chan, cmd)
        });

        debug!("spawned thread for command.");

        Ok(SshSpawnHandle { thread_handle })
    }
}

impl SshSpawnHandle {
    /// Block until the remote command completes.
    pub fn join(self) -> Result<SshOutput, failure::Error> {
        debug!("Blocking on spawned commmand.");
        self.thread_handle.join().unwrap()
    }
}

/// A useful macro that allows creating commands with format strings and arguments.
///
/// ```rust,ignore
/// cmd!("ls {}", "foo")
/// ```
///
/// is equivalent to the expression
///
/// ```rust,ignore
/// SshCommand::new(&format!("ls {}", "foo"))
/// ```
#[macro_export]
macro_rules! cmd {
    ($fmt:expr) => {
        $crate::ssh::SshCommand::new(&format!($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::ssh::SshCommand::new(&format!($fmt, $($arg)*))
    };
}
