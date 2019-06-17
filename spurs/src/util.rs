//! Useful utilities for running commands.

use std::net::{IpAddr, ToSocketAddrs};

use crate::ssh::Execute;

///////////////////////////////////////////////////////////////////////////////
// Common useful routines
///////////////////////////////////////////////////////////////////////////////

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

/// Given a host:ip address, return `(host, ip)`.
pub fn get_host_ip<A: ToSocketAddrs>(addr: A) -> (IpAddr, u16) {
    let addr = addr.to_socket_addrs().unwrap().next().unwrap();
    let ip = addr.ip();
    let port = addr.port();
    (ip, port)
}

/// Reboot and wait for the remote machine to come back up again. Requires `sudo`.
pub fn reboot(shell: &mut impl Execute, dry_run: bool) -> Result<(), failure::Error> {
    let _ = shell.run(cmd!("sudo reboot").dry_run(dry_run));

    if !dry_run {
        // If we try to reconnect immediately, the machine will not have gone down yet.
        #[cfg(not(test))]
        std::thread::sleep(std::time::Duration::from_secs(10));

        // Attempt to reconnect.
        shell.reconnect()?;
    }

    // Make sure it worked.
    shell.run(cmd!("whoami").dry_run(dry_run))?;

    Ok(())
}

///////////////////////////////////////////////////////////////////////////////
// Tests
///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use log::info;

    use crate::ssh::{Execute, SshCommand, SshOutput};

    /// An `Execute` implementation for use in tests.
    pub struct TestSshShell {
        pub commands: std::sync::Mutex<Vec<SshCommand>>,
    }

    impl TestSshShell {
        pub fn new() -> Self {
            // init logging if never done before...
            use std::sync::Once;
            static START: Once = Once::new();
            START.call_once(|| {
                env_logger::init();
            });

            Self {
                commands: std::sync::Mutex::new(vec![]),
            }
        }
    }

    /// A spawn handle for use in tests.
    pub struct TestSshSpawnHandle {
        pub command: SshCommand,
    }

    impl Execute for TestSshShell {
        type SshSpawnHandle = TestSshSpawnHandle;

        fn run(&self, cmd: SshCommand) -> Result<SshOutput, failure::Error> {
            info!("Test run({:#?})", cmd);

            enum FakeCommand {
                Blkid,
                Kname1,
                Kname2,
                Kname3,
                Kname4,
                KnameMountpoint,
                Size1,
                Size2,
                Size3,
                Unknown,
            }

            let short_cmd = {
                if cmd.cmd().contains("blkid") {
                    FakeCommand::Blkid
                } else if cmd.cmd().contains("KNAME /dev/foobar") {
                    FakeCommand::Kname1
                } else if cmd.cmd().contains("KNAME /dev/sd") {
                    FakeCommand::Kname3
                } else if cmd.cmd().contains("KNAME /dev/") {
                    FakeCommand::Kname4
                } else if cmd.cmd().contains("KNAME,MOUNTPOINT") {
                    FakeCommand::KnameMountpoint
                } else if cmd.cmd().contains("KNAME") {
                    FakeCommand::Kname2
                } else if cmd.cmd().contains("SIZE /dev/sda") {
                    FakeCommand::Size1
                } else if cmd.cmd().contains("SIZE /dev/sdb") {
                    FakeCommand::Size2
                } else if cmd.cmd().contains("SIZE /dev/sdc") {
                    FakeCommand::Size3
                } else {
                    FakeCommand::Unknown
                }
            };

            self.commands.lock().unwrap().push(cmd);

            let stdout = match short_cmd {
                FakeCommand::Blkid => "UUID=1fb958bf-de7e-428a-a0b7-a598f22e96fa\n".into(),
                FakeCommand::Kname1 => "KNAME\nfoobar\nfoo\nbar\nbaz\n".into(),
                FakeCommand::Kname2 => "KNAME\nfoobar\nfoo\nbar\nbaz\nsdb\nsdc".into(),
                FakeCommand::Kname3 => "KNAME\nsdb".into(),
                FakeCommand::Kname4 => "KNAME\nfoo".into(),
                FakeCommand::KnameMountpoint => {
                    "KNAME MOUNTPOINT\nfoobar\nfoo  /mnt/foo\nbar  /mnt/bar\nbaz\nsdb\nsdc".into()
                }
                FakeCommand::Size1 => "SIZE\n477G".into(),
                FakeCommand::Size2 => "SIZE\n400G".into(),
                FakeCommand::Size3 => "SIZE\n500G".into(),
                FakeCommand::Unknown => String::new(),
            };

            info!("Output: {}", stdout);

            Ok(SshOutput {
                stdout,
                stderr: String::new(),
            })
        }

        fn spawn(&self, cmd: SshCommand) -> Result<Self::SshSpawnHandle, failure::Error> {
            info!("Test spawn({:#?})", cmd);
            Ok(TestSshSpawnHandle { command: cmd })
        }

        fn reconnect(&mut self) -> Result<(), failure::Error> {
            info!("Test reconnect");

            Ok(())
        }
    }

    macro_rules! expect_cmd_sequence {
        ($shell:expr) => {
            assert!($shell.commands.is_empty());
        };
        ($shell:expr, $($cmd:expr),+ $(,)?) => {
            let expected: &[SshCommand] = &[$($cmd),+];
            let locked = $shell.commands.lock().unwrap();

            if locked.len() != expected.len() {
                panic!("Number of commands run does not match expected number: \n Expected: {:#?}\nActual:  {:#?}====\n", expected, locked);
            }

            let mut fail = false;
            let mut message = "Actual commands did not match expected commands: \n".to_owned();

            for (expected, actual) in expected.iter().zip(locked.iter()) {
                if expected != actual {
                    fail = true;
                    message.push_str(&format!("\nExpected: {:#?}\nActual:  {:#?}\n=====\n", expected, actual));
                }
            }

            if fail {
                panic!(message);
            }
        };
    }

    mod test_escape_for_bash {
        use super::super::escape_for_bash;

        #[test]
        fn simple() {
            const TEST_STRING: &str = "ls";
            assert_eq!(escape_for_bash(TEST_STRING), "'ls'");
        }

        #[test]
        fn more_complex() {
            const TEST_STRING: &str = r#"echo '$HELLOWORLD="hello world"' | grep "hello""#;
            assert_eq!(
                escape_for_bash(TEST_STRING),
                r#"'echo '"'"'$HELLOWORLD="hello world"'"'"' | grep "hello"'"#
            );
        }
    }

    #[test]
    fn test_get_host_ip() {
        const TEST_ADDR: &str = "localhost:2303";
        let (addr, port) = super::get_host_ip(TEST_ADDR);

        assert_eq!(addr, "127.0.0.1".parse::<std::net::IpAddr>().unwrap());
        assert_eq!(port, 2303);
    }

    #[test]
    fn test_reboot() {
        let mut shell = TestSshShell::new();
        super::reboot(&mut shell, false).unwrap();
        expect_cmd_sequence! {
            shell,
            SshCommand::make_cmd("sudo reboot", None, false, false, false, false),
            SshCommand::make_cmd("whoami", None, false, false, false, false),
        };
    }
}
