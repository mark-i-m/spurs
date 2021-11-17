//! `spurs` is a library for executing commands remotely over SSH. I created it in an effort to
//! automate setup and experimentation on a cluster of machines.
//!
//! `spurs` prioritizes ergonomics over performance. It is _not_ a high-performance way of getting
//! stuff done in a cluster.
//!
//! `spurs` takes heavy inspiration from the python
//! [spur.py](https://github.com/mwilliamson/spur.py) library, which is amazing. At some point,
//! though, my scripts were so big that python was getting in my way, so I created `spurs` to allow
//! me to build my cluster setup/experiments scripts/framework in rust, with much greater
//! productivity and refactorability.

#![doc(html_root_url = "https://docs.rs/spurs/0.9.2")]

use std::{
    io::Read,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use log::{debug, info, trace};

use ssh2::Session;

/// The default timeout for the TCP stream of a SSH connection.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
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

/// An error type representing things that could possibly go wrong when using an SshShell.
#[derive(Debug)]
pub enum SshError {
    /// Unable to find the private key at the given path.
    KeyNotFound { file: String },

    /// SSH authentication failed.
    AuthFailed { key: std::path::PathBuf },

    /// The comand run over SSH returned with a non-zero exit code.
    NonZeroExit { cmd: String, exit: i32 },

    /// An SSH error occurred.
    SshError { error: ssh2::Error },

    /// An I/O error occurred.
    IoError { error: std::io::Error },
}

/// Represents a connection via SSH to a particular source.
pub struct SshShell {
    // The TCP stream needs to be in the struct to keep it alive while the session is active.
    tcp: TcpStream,
    username: String,
    key: PathBuf,
    remote_name: String, // used for printing
    remote: SocketAddr,
    sess: Arc<Mutex<Session>>,
    dry_run_mode: bool,
}

/// A handle for a spawned remote command.
pub struct SshSpawnHandle {
    thread_handle: JoinHandle<(SshShell, Result<SshOutput, SshError>)>,
}

/// A trait representing types that can run an `SshCommand`.
pub trait Execute: Sized {
    /// Run a command on the remote machine, blocking until the command completes.
    ///
    /// Note that command using `sudo` will hang indefinitely if `sudo` asks for a password.
    fn run(&self, cmd: SshCommand) -> Result<SshOutput, SshError>;

    /// Attempts to create a new `Self` with similar credentials to `self` but using an independent
    /// connection. This is useful for running multiple commands in parallel without needing to
    /// pass around the parameters everywhere.
    fn duplicate(&self) -> Result<Self, SshError>;

    /// Attempt to reconnect to the remote until it reconnects (possibly indefinitely).
    fn reconnect(&mut self) -> Result<(), SshError>;
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            SshError::KeyNotFound { file } => write!(f, "no such key: {}", file),
            SshError::AuthFailed { key } => {
                write!(f, "authentication failed with private key: {:?}", key)
            }
            SshError::NonZeroExit { cmd, exit } => {
                write!(f, "non-zero exit ({}) for command: {}", exit, cmd)
            }
            SshError::SshError { error } => write!(f, "{}", error),
            SshError::IoError { error } => write!(f, "{}", error),
        }
    }
}

impl std::error::Error for SshError {}

impl std::convert::From<ssh2::Error> for SshError {
    fn from(error: ssh2::Error) -> Self {
        SshError::SshError { error }
    }
}

impl std::convert::From<std::io::Error> for SshError {
    fn from(error: std::io::Error) -> Self {
        SshError::IoError { error }
    }
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

    /// Helper for tests that makes a `SshCommand` with the given values.
    #[cfg(any(test, feature = "test"))]
    pub fn make_cmd(
        cmd: &str,
        cwd: Option<PathBuf>,
        use_bash: bool,
        allow_error: bool,
        dry_run: bool,
        no_pty: bool,
    ) -> Self {
        SshCommand {
            cmd: cmd.into(),
            cwd,
            use_bash,
            allow_error,
            dry_run,
            no_pty,
        }
    }

    /// Helper for tests to get the command from this `SshCommand`.
    #[cfg(any(test, feature = "test"))]
    pub fn cmd(&self) -> &str {
        &self.cmd
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
    ) -> Result<Self, SshError> {
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

    /// Returns a shell connected via the first private key found at `$HOME/.ssh/` to the given
    /// SSH server as the given user.
    ///
    /// ```rust,ignore
    /// SshShell::with_any_key("markm", "myhost:22")?;
    /// ```
    pub fn with_any_key<A: Copy + ToSocketAddrs + std::fmt::Debug>(
        username: &str,
        remote: A,
    ) -> Result<Self, SshError> {
        const DEFAULT_KEY_DIR: &str = ".ssh/";
        let home = if let Some(home) = dirs::home_dir() {
            home
        } else {
            return Err(SshError::KeyNotFound {
                file: DEFAULT_KEY_DIR.into(),
            });
        };
        let key_dir = home.join(DEFAULT_KEY_DIR);

        for entry in std::fs::read_dir(&key_dir)? {
            let entry = entry?;
            let name = entry.file_name().into_string().unwrap();

            // To find the private keys, find the public keys then chop off ".pub"
            if !name.ends_with(".pub") {
                continue;
            }

            let (priv_key, _) = name.split_at(name.len() - 4);
            let shell = SshShell::with_key(username, remote, key_dir.join(priv_key));

            if shell.is_ok() {
                return shell;
            }
        }

        Err(SshError::KeyNotFound {
            file: DEFAULT_KEY_DIR.into(),
        })
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
    ) -> Result<Self, SshError> {
        info!("New SSH shell: {}@{:?}", username, remote);
        debug!("Using key: {:?}", key.as_ref());

        debug!("Create new TCP stream...");

        // Create a TCP connection
        let tcp = TcpStream::connect(&remote)?;
        tcp.set_read_timeout(Some(DEFAULT_TIMEOUT))?;
        tcp.set_write_timeout(Some(DEFAULT_TIMEOUT))?;
        let remote_name = format!("{:?}", remote);
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
            console::style(format!("{}@{} ({})", username, remote_name, remote))
                .green()
                .bold()
        );

        Ok(SshShell {
            tcp,
            username: username.to_owned(),
            key: key.as_ref().to_owned(),
            remote_name,
            remote,
            sess: Arc::new(Mutex::new(sess)),
            dry_run_mode: false,
        })
    }

    /// Returns a new shell connected via the same credentials as the given existing host.
    ///
    /// ```rust,ignore
    /// SshShell::from_existing(&existing_ssh_shell)?;
    /// ```
    pub fn from_existing(shell: &SshShell) -> Result<Self, SshError> {
        info!("New SSH shell: {}@{:?}", shell.username, shell.remote);
        debug!("Using key: {:?}", shell.key);

        debug!("Create new TCP stream...");

        // Create a TCP connection
        let tcp = TcpStream::connect(&shell.remote)?;
        tcp.set_read_timeout(Some(DEFAULT_TIMEOUT))?;
        tcp.set_write_timeout(Some(DEFAULT_TIMEOUT))?;
        let remote = shell.remote.clone();

        debug!("Create new SSH session...");

        // Start an SSH session
        let mut sess = Session::new().unwrap();
        sess.handshake(&tcp)?;
        trace!("SSH session handshook.");
        sess.userauth_pubkey_file(&shell.username, None, shell.key.as_ref(), None)?;
        if !sess.authenticated() {
            return Err(SshError::AuthFailed {
                key: shell.key.clone(),
            }
            .into());
        }
        trace!("SSH session authenticated.");

        println!(
            "{}",
            console::style(format!(
                "{}@{} ({})",
                shell.username, shell.remote_name, remote
            ))
            .green()
            .bold()
        );

        Ok(SshShell {
            tcp,
            username: shell.username.clone(),
            key: shell.key.clone(),
            remote_name: shell.remote_name.clone(),
            remote,
            sess: Arc::new(Mutex::new(sess)),
            dry_run_mode: false,
        })
    }

    /// Toggles _dry run mode_. In dry run mode, commands are not executed remotely; we only print
    /// what commands we would execute. Note that we do connect remotely, though. This is off by
    /// default: we default to actually running the commands.
    pub fn set_dry_run(&mut self, on: bool) {
        self.dry_run_mode = on;
        info!(
            "Toggled dry run mode: {}",
            if self.dry_run_mode { "on" } else { "off" }
        );
    }

    pub fn spawn(&self, cmd: SshCommand) -> Result<SshSpawnHandle, SshError> {
        debug!("spawn({:?})", cmd);
        let shell = Self::from_existing(self)?;
        let cmd = if self.dry_run_mode {
            cmd.dry_run(true)
        } else {
            cmd
        };

        let thread_handle = std::thread::spawn(move || {
            let result = shell.run(cmd);
            (shell, result)
        });

        debug!("spawned thread for command.");

        Ok(SshSpawnHandle { thread_handle })
    }

    fn run_with_chan_and_opts(
        host_and_username: String, // for printing
        mut chan: ssh2::Channel,
        cmd_opts: SshCommand,
    ) -> Result<SshOutput, SshError> {
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
            format!("bash -c {}", escape_for_bash(&cmd))
        } else {
            cmd
        };

        debug!("After shell escaping: {:?}", cmd);

        let cmd = if let Some(cwd) = &cwd {
            format!("cd {} ; {}", cwd.display(), cmd)
        } else {
            cmd
        };

        debug!("After cwd: {:?}", cmd);

        // print message
        if let Some(cwd) = cwd {
            println!(
                "{:-<80}\n{}\n{}\n{}",
                "",
                console::style(host_and_username).blue(),
                console::style(cwd.display()).blue(),
                console::style(msg).yellow().bold()
            );
        } else {
            println!(
                "{:-<80}\n{}\n{}",
                "",
                console::style(host_and_username).blue(),
                console::style(msg).yellow().bold()
            );
        }

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
            let out = String::from_utf8_lossy(&buf);
            let out = out.trim_end_matches('\u{0}');
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
            let err = String::from_utf8_lossy(&buf);
            let err = err.trim_end_matches('\u{0}');
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
}

impl Execute for SshShell {
    fn run(&self, cmd: SshCommand) -> Result<SshOutput, SshError> {
        debug!("run(cmd)");
        let sess = self.sess.lock().unwrap();
        debug!("Attempt to crate channel...");
        let chan = sess.channel_session()?;
        debug!("Channel created.");
        let host_and_username = format!("{}@{}", self.username, self.remote_name);
        let cmd = if self.dry_run_mode {
            cmd.dry_run(true)
        } else {
            cmd
        };
        Self::run_with_chan_and_opts(host_and_username, chan, cmd)
    }

    fn duplicate(&self) -> Result<Self, SshError> {
        Self::from_existing(self)
    }

    fn reconnect(&mut self) -> Result<(), SshError> {
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

        // It should be safe to `Arc::get_mut` here. `reconnect` takes `self` by mutable reference,
        // so no other thread should have access (even immutably) to `self.sess`.
        let self_sess = Arc::get_mut(&mut self.sess).unwrap().get_mut().unwrap();
        let _old_sess = std::mem::replace(self_sess, sess);

        println!(
            "{}",
            console::style(format!("{}@{}", self.username, self.remote))
                .green()
                .bold()
        );

        Ok(())
    }
}

impl std::fmt::Debug for SshShell {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "SshShell {{ {}@{:?} dry_run={} key={:?} }}",
            self.username, self.remote, self.dry_run_mode, self.key
        )
    }
}

impl SshSpawnHandle {
    /// Block until the remote command completes.
    pub fn join(self) -> (SshShell, Result<SshOutput, SshError>) {
        debug!("Blocking on spawned commmand.");
        let ret = self.thread_handle.join().unwrap();
        debug!("Spawned commmand complete.");
        ret
    }
}

impl std::fmt::Debug for SshSpawnHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "SshSpawnHandle {{ running }}")
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
        $crate::SshCommand::new(&format!($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::SshCommand::new(&format!($fmt, $($arg)*))
    };
}

/// Given a string, properly escape the string so that it can be passed as a command line argument
/// to bash.
///
/// This is useful for passing commands to `bash -c` (e.g. through ssh).
fn escape_for_bash(s: &str) -> String {
    let mut new = String::with_capacity(s.len());

    // Escape every non-alphanumeric character.
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            new.push(c);
        } else {
            new.push('\\');
            new.push(c);
        }
    }

    new
}

///////////////////////////////////////////////////////////////////////////////
// Tests
///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use crate::{cmd, SshCommand};

    #[test]
    fn test_cmd_macro() {
        assert_eq!(cmd!("{} {}", "ls", 3), SshCommand::new("ls 3"));
    }

    mod test_escape_for_bash {
        use super::super::escape_for_bash;

        #[test]
        fn simple() {
            const TEST_STRING: &str = "ls";
            assert_eq!(escape_for_bash(TEST_STRING), "ls");
        }

        #[test]
        fn more_complex() {
            use std::process::Command;

            const TEST_STRING: &str =
                r#""Bob?!", said she, "I though you said 'I can't be there'!""#;

            let out = Command::new("bash")
                .arg("-c")
                .arg(&format!("echo {}", escape_for_bash(TEST_STRING)))
                .output()
                .unwrap();
            let out = String::from_utf8(out.stdout).unwrap();

            assert_eq!(out.trim(), TEST_STRING);
        }
    }
}
