//! This is a library containing a bunch of routines that I have found useful for setting up and
//! running experiments remotely.
//!
//! Some of these utilities execute a sequence of steps. They require a shell as input and actually
//! run a command remotely.
//!
//! The rest only construct a command that can be executed and return it to the caller _without
//! executing anything_.
//!
//! There are also some utilities that don't construct or run commands. They are just useful
//! functions for constructing commands.
//!
//! The `centos` and `ubuntu` submodules contain routines specifically useful for those platforms.

#![doc(html_root_url = "https://docs.rs/spurs-util/0.2.1")]

pub mod centos;
pub mod ubuntu;

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::{IpAddr, ToSocketAddrs},
};

use spurs::{
    cmd,
    errors::SshError,
    ssh::{Execute, SshCommand},
};

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

///////////////////////////////////////////////////////////////////////////////
// Below are utilies that just construct (but don't run) a command.
///////////////////////////////////////////////////////////////////////////////

/// Sets the CPU scaling governor to the given governor. This requires
/// - `cpupower` to be installed,
/// - `sudo` priveleges,
/// - the necessary Linux kernel modules.
pub fn set_cpu_scaling_governor(gov: &str) -> SshCommand {
    cmd!("sudo cpupower frequency-set -g {}", gov)
}

/// Turn off the swap device. Requires `sudo` permissions.
pub fn swapoff(device: &str) -> SshCommand {
    cmd!("sudo swapoff {}", device)
}

/// Turn on the swap device. Requires `sudo` permissions. Assumes the device is already formatted
/// as a swap device (i.e. with `mkswap`).
pub fn swapon(device: &str) -> SshCommand {
    cmd!("sudo swapon {}", device)
}

/// Add the executing user to the given group. Requires `sudo` permissions.
pub fn add_to_group(group: &str) -> SshCommand {
    cmd!("sudo usermod -aG {} `whoami`", group).use_bash()
}

/// Write a new general partition table (GPT) on the given device. Requires `sudo` permissions.
///
/// **NOTE**: this will destroy any data on the partition!
pub fn write_gpt(device: &str) -> SshCommand {
    cmd!("sudo parted -a optimal {} -s -- mklabel gpt", device)
}

/// Create a new partition on the given device. Requires `sudo` permissions.
pub fn create_partition(device: &str) -> SshCommand {
    cmd!(
        "sudo parted -a optimal {} -s -- mkpart primary 0% 100%",
        device
    )
}

///////////////////////////////////////////////////////////////////////////////
// Below are utilies that actually run a command. These require a shell as input.
///////////////////////////////////////////////////////////////////////////////

/// Formats and mounts the given device as ext4 at the given mountpoint owned by the given user.
/// The given partition and mountpoint are assumed to be valid (we don't check).  We will assume
/// quite a few things for simplicity:
/// - the disk _IS_ partitioned, but the partition is not formatted
/// - the disk should be mounted at the mountpoint, which is a valid directory
/// - you have `sudo` permissions
/// - `owner` is a valid username
///
/// We need to be careful not to mess up the ssh keys, so we will first mount the
/// new FS somewhere, copy over dotfiles, then unmount and mount to users...
///
/// In particular, this is useful for mounting a new partition as a home directory.
///
/// # Warning!
///
/// This can cause data loss and seriously mess up your system. **BE VERY CAREFUL**. Make sure you
/// are formatting the write partition.
///
/// # Example
///
/// ```rust,ignore
/// format_partition_as_ext4(root_shell, "/dev/sda4", "/home/foouser/")?;
/// ```
pub fn format_partition_as_ext4<P: AsRef<std::path::Path>>(
    shell: &impl Execute,
    dry_run: bool,
    partition: &str,
    mount: P,
    owner: &str,
) -> Result<(), SshError> {
    shell.run(cmd!("lsblk").dry_run(dry_run))?;

    // Make a filesystem on the first partition
    shell.run(cmd!("sudo mkfs.ext4 {}", partition).dry_run(dry_run))?;

    // Mount the FS in tmp
    shell.run(cmd!("mkdir -p /tmp/tmp_mnt").dry_run(dry_run))?;
    shell.run(cmd!("sudo mount -t ext4 {} /tmp/tmp_mnt", partition).dry_run(dry_run))?;
    shell.run(cmd!("sudo chown {} /tmp/tmp_mnt", owner).dry_run(dry_run))?;

    // Copy all existing files
    shell.run(cmd!("rsync -a {}/ /tmp/tmp_mnt/", mount.as_ref().display()).dry_run(dry_run))?;

    // Unmount from tmp
    shell.run(cmd!("sync").dry_run(dry_run))?;
    shell.run(cmd!("sudo umount /tmp/tmp_mnt").dry_run(dry_run))?;

    // Mount the FS at `mount`
    shell.run(
        cmd!(
            "sudo mount -t ext4 {} {}",
            partition,
            mount.as_ref().display()
        )
        .dry_run(dry_run),
    )?;
    shell.run(cmd!("sudo chown {} {}", owner, mount.as_ref().display()).dry_run(dry_run))?;

    // Add to /etc/fstab
    let uuid = shell
        .run(
            cmd!("sudo blkid -o export {} | grep '^UUID='", partition)
                .use_bash()
                .dry_run(dry_run),
        )?
        .stdout;
    let uuid = uuid.trim();
    shell.run(
        cmd!(
            r#"echo "{}    {}    ext4    defaults    0    1" | sudo tee -a /etc/fstab"#,
            uuid,
            mount.as_ref().display()
        )
        .dry_run(dry_run),
    )?;

    // Print for info
    shell.run(cmd!("lsblk").dry_run(dry_run))?;

    Ok(())
}

/// Returns a list of partitions of the given device. For example, `["sda1", "sda2"]`.
pub fn get_partitions(
    shell: &impl Execute,
    device: &str,
    dry_run: bool,
) -> Result<HashSet<String>, SshError> {
    Ok(shell
        .run(cmd!("lsblk -o KNAME {}", device).dry_run(dry_run))?
        .stdout
        .lines()
        .map(|line| line.trim().to_owned())
        .skip(2)
        .collect())
}

/// Returns a list of devices with no partitions. For example, `["sda", "sdb"]`.
pub fn get_unpartitioned_devs(
    shell: &impl Execute,
    dry_run: bool,
) -> Result<HashSet<String>, SshError> {
    // List all devs
    let lsblk = shell.run(cmd!("lsblk -o KNAME").dry_run(dry_run))?.stdout;
    let mut devices: BTreeSet<&str> = lsblk.lines().map(|line| line.trim()).skip(1).collect();

    // Get the partitions of each device.
    let partitions: HashMap<_, _> = devices
        .iter()
        .map(|&dev| {
            (
                dev,
                get_partitions(shell, &format!("/dev/{}", dev), dry_run),
            )
        })
        .collect();

    // Remove partitions and partitioned devices from the list of devices
    for (dev, parts) in partitions.into_iter() {
        let parts = parts?;
        if !parts.is_empty() {
            devices.remove(dev);
            for part in parts {
                devices.remove(part.as_str());
            }
        }
    }

    Ok(devices.iter().map(|&dev| dev.to_owned()).collect())
}

/// Returns the list of devices mounted and their mountpoints. For example, `[("sda2", "/")]`.
pub fn get_mounted_devs(
    shell: &impl Execute,
    dry_run: bool,
) -> Result<Vec<(String, String)>, SshError> {
    let devices = shell
        .run(cmd!("lsblk -o KNAME,MOUNTPOINT").dry_run(dry_run))?
        .stdout;
    let devices = devices.lines().skip(1);
    let mut mounted = vec![];
    for line in devices {
        let split: Vec<_> = line
            .split(char::is_whitespace)
            .filter(|s| !s.is_empty())
            .collect();

        // Need to make sure there are no duplicates (which can happen with LVM)
        if split.len() > 1 && mounted.iter().all(|(d, _)| d != split[0]) {
            mounted.push((split[0].to_owned(), split[1].to_owned()));
        }
    }
    Ok(mounted)
}

/// Returns the human-readable size of the devices `devs`. For example, `["477G", "500M"]`.
pub fn get_dev_sizes(
    shell: &impl Execute,
    devs: Vec<&str>,
    dry_run: bool,
) -> Result<Vec<String>, SshError> {
    let per_dev = devs
        .iter()
        .map(|dev| shell.run(cmd!("lsblk -o SIZE /dev/{}", dev).dry_run(dry_run)));

    let mut sizes = vec![];
    for size in per_dev {
        sizes.push(size?.stdout.lines().nth(1).unwrap().trim().to_owned());
    }

    Ok(sizes)
}

/// Reboot and wait for the remote machine to come back up again. Requires `sudo`.
pub fn reboot(shell: &mut impl Execute, dry_run: bool) -> Result<(), SshError> {
    let _ = shell.run(cmd!("sudo reboot").dry_run(dry_run));

    if !dry_run {
        // If we try to reconnect immediately, the machine will not have gone down yet.
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

    use spurs::errors::SshError;
    use spurs::ssh::{Execute, SshCommand, SshOutput};

    /// An `Execute` implementation for use in tests.
    #[derive(Clone, Debug)]
    pub struct TestSshShell {
        pub commands: std::sync::Arc<std::sync::Mutex<Vec<SshCommand>>>,
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
                commands: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            }
        }
    }

    /// A spawn handle for use in tests.
    pub struct TestSshSpawnHandle {
        pub command: SshCommand,
    }

    impl Execute for TestSshShell {
        type SshSpawnHandle = TestSshSpawnHandle;

        fn run(&self, cmd: SshCommand) -> Result<SshOutput, SshError> {
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

        fn spawn(&self, cmd: SshCommand) -> Result<(Self, Self::SshSpawnHandle), SshError> {
            info!("Test spawn({:#?})", cmd);
            Ok((self.clone(), TestSshSpawnHandle { command: cmd }))
        }

        fn reconnect(&mut self) -> Result<(), SshError> {
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

    #[test]
    fn test_set_cpu_scaling_governor() {
        assert_eq!(
            super::set_cpu_scaling_governor("foobar"),
            SshCommand::make_cmd(
                "sudo cpupower frequency-set -g foobar".into(),
                None,
                false,
                false,
                false,
                false,
            )
        );
    }

    #[test]
    fn test_swapoff() {
        assert_eq!(
            super::swapoff("foobar"),
            SshCommand::make_cmd(
                "sudo swapoff foobar".into(),
                None,
                false,
                false,
                false,
                false,
            )
        );
    }

    #[test]
    fn test_swapon() {
        assert_eq!(
            super::swapon("foobar"),
            SshCommand::make_cmd(
                "sudo swapon foobar".into(),
                None,
                false,
                false,
                false,
                false,
            )
        );
    }

    #[test]
    fn test_add_to_group() {
        assert_eq!(
            super::add_to_group("foobar"),
            SshCommand::make_cmd(
                "sudo usermod -aG foobar `whoami`".into(),
                None,
                true, // use_bash
                false,
                false,
                false,
            )
        );
    }

    #[test]
    fn test_write_gpt() {
        assert_eq!(
            super::write_gpt("foobar"),
            SshCommand::make_cmd(
                "sudo parted -a optimal foobar -s -- mklabel gpt".into(),
                None,
                false,
                false,
                false,
                false,
            )
        );
    }

    #[test]
    fn test_create_partition() {
        assert_eq!(
            super::create_partition("foobar"),
            SshCommand::make_cmd(
                "sudo parted -a optimal foobar -s -- mkpart primary 0% 100%".into(),
                None,
                false,
                false,
                false,
                false,
            )
        );
    }

    #[test]
    fn test_format_partition_as_ext4() {
        let mut shell = TestSshShell::new();
        super::format_partition_as_ext4(&mut shell, false, "/dev/foobar", "/mnt/point/", "me")
            .unwrap();
        expect_cmd_sequence! {
            shell,
            SshCommand::make_cmd("lsblk", None, false, false, false, false),
            SshCommand::make_cmd("sudo mkfs.ext4 /dev/foobar", None, false, false, false, false),
            SshCommand::make_cmd("mkdir -p /tmp/tmp_mnt", None, false, false, false, false),
            SshCommand::make_cmd("sudo mount -t ext4 /dev/foobar /tmp/tmp_mnt", None, false, false, false, false),
            SshCommand::make_cmd("sudo chown me /tmp/tmp_mnt", None, false, false, false, false),
            SshCommand::make_cmd("rsync -a /mnt/point// /tmp/tmp_mnt/", None, false, false, false, false),
            SshCommand::make_cmd("sync", None, false, false, false, false),
            SshCommand::make_cmd("sudo umount /tmp/tmp_mnt", None, false, false, false, false),
            SshCommand::make_cmd("sudo mount -t ext4 /dev/foobar /mnt/point/", None, false, false, false, false),
            SshCommand::make_cmd("sudo chown me /mnt/point/", None, false, false, false, false),
            SshCommand::make_cmd("sudo blkid -o export /dev/foobar | grep '^UUID='", None, /* use_bash = */ true, false, false, false),
            SshCommand::make_cmd(r#"echo "UUID=1fb958bf-de7e-428a-a0b7-a598f22e96fa    /mnt/point/    ext4    defaults    0    1" | sudo tee -a /etc/fstab"#, None, false, false, false, false),
            SshCommand::make_cmd("lsblk", None, false, false, false, false),
        };
    }

    #[test]
    fn test_get_partitions() {
        let mut shell = TestSshShell::new();
        let partitions = super::get_partitions(&mut shell, "/dev/foobar", false).unwrap();
        expect_cmd_sequence! {
            shell,
            SshCommand::make_cmd("lsblk -o KNAME /dev/foobar", None, false, false, false, false),
        }
        assert_eq!(
            {
                let mut set = std::collections::HashSet::new();
                set.insert("foo".into());
                set.insert("bar".into());
                set.insert("baz".into());
                set
            },
            partitions
        );
    }

    #[test]
    fn test_get_unpartitioned_devices() {
        let mut shell = TestSshShell::new();
        let devs = super::get_unpartitioned_devs(&mut shell, false).unwrap();
        expect_cmd_sequence! {
            shell,
            SshCommand::make_cmd("lsblk -o KNAME", None, false, false, false, false),
            SshCommand::make_cmd("lsblk -o KNAME /dev/bar", None, false, false, false, false),
            SshCommand::make_cmd("lsblk -o KNAME /dev/baz", None, false, false, false, false),
            SshCommand::make_cmd("lsblk -o KNAME /dev/foo", None, false, false, false, false),
            SshCommand::make_cmd("lsblk -o KNAME /dev/foobar", None, false, false, false, false),
            SshCommand::make_cmd("lsblk -o KNAME /dev/sdb", None, false, false, false, false),
            SshCommand::make_cmd("lsblk -o KNAME /dev/sdc", None, false, false, false, false),
        }
        assert_eq!(
            {
                let mut set = std::collections::HashSet::new();
                set.insert("sdb".into());
                set.insert("sdc".into());
                set
            },
            devs
        );
    }

    #[test]
    fn test_get_mounted_devs() {
        let mut shell = TestSshShell::new();
        let devs = super::get_mounted_devs(&mut shell, false).unwrap();
        expect_cmd_sequence! {
            shell,
            SshCommand::make_cmd("lsblk -o KNAME,MOUNTPOINT", None, false, false, false, false),
        }
        assert_eq!(
            vec![
                ("foo".to_owned(), "/mnt/foo".to_owned()),
                ("bar".to_owned(), "/mnt/bar".to_owned())
            ],
            devs
        );
    }

    #[test]
    fn test_get_dev_sizes() {
        let mut shell = TestSshShell::new();
        let devs = super::get_dev_sizes(&mut shell, vec!["sda", "sdb", "sdc"], false).unwrap();
        expect_cmd_sequence! {
            shell,
            SshCommand::make_cmd("lsblk -o SIZE /dev/sda", None, false, false, false, false),
            SshCommand::make_cmd("lsblk -o SIZE /dev/sdb", None, false, false, false, false),
            SshCommand::make_cmd("lsblk -o SIZE /dev/sdc", None, false, false, false, false),
        }
        assert_eq!(vec!["477G".to_owned(), "400G".into(), "500G".into()], devs);
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
