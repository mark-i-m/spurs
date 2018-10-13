//! A collection of useful utilities for running commands, configuring machines, etc.

use crate::ssh::{SshCommand, SshShell};

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

/// Sets the CPU scaling governor to the given governor. This requires
/// - `cpupower` to be installed,
/// - `sudo` priveleges,
/// - the necessary Linux kernel modules.
pub fn set_cpu_scaling_governor(shell: SshShell, gov: &str) -> Result<(), failure::Error> {
    shell
        .run(cmd!("sudo cpupower frequency-set -g {}", gov))
        .map(|_| ())
}

/// Formats and mounts the given device as ext4 at the given mountpoint owned by the given user.
/// The given partition and mountpoint are assumed to be valid (we don't check).  We will assume
/// quite a few things for simplicity:
/// - the disk _IS_ partitioned, but the partition is not formatted
/// - the disk is mounted at the mountpoint, which is a valid directory
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
    shell: SshShell,
    partition: &str,
    mount: P,
    owner: &str,
) -> Result<(), failure::Error> {
    shell.run(cmd!("lsblk"))?;

    // Format partition
    shell.run(cmd!(
        "sudo parted -a optimal {} -s -- mkpart primary 0%% 100%%",
        partition
    ))?;

    // Make a filesystem on the first partition
    shell.run(cmd!("sudo mkfs.ext4 {}", partition))?;

    // Mount the FS in tmp
    shell.run(cmd!("mkdir -p /tmp/tmp_mnt"))?;
    shell.run(cmd!("sudo mount -t ext4 {} /tmp/tmp_mnt", partition))?;
    shell.run(cmd!("sudo chown {} /tmp/tmp_mnt", owner))?;

    // Copy all existing files
    shell.run(cmd!("rsync -a {} /tmp/tmp_mnt/", mount.as_ref().display()))?;

    // Unmount from tmp
    shell.run(cmd!("sync"))?;
    shell.run(cmd!("sudo umount /tmp/tmp_mnt"))?;

    // Mount the FS at `mount`
    shell.run(cmd!(
        "sudo mount -t ext4 {} {}",
        partition,
        mount.as_ref().display()
    ))?;
    shell.run(cmd!("sudo chown {} {}", owner, mount.as_ref().display()))?;

    // Add to /etc/fstab
    let uuid = shell
        .run(cmd!("sudo blkid -o export {} | grep UUID", partition).use_bash())?
        .stdout;
    shell.run(cmd!(
        r#"echo "{}    {}    ext4    defaults    0    1" | sudo tee -a /etc/fstab"#,
        uuid,
        mount.as_ref().display()
    ))?;

    // Print for info
    shell.run(cmd!("lsblk"))?;

    Ok(())
}

/// Turn off the swap device. Requires `sudo` permissions.
pub fn swapoff(shell: &mut SshShell, device: &str) -> Result<(), failure::Error> {
    shell.run(cmd!("sudo swapoff {}", device)).map(|_| ())
}

/// Turn on the swap device. Requires `sudo` permissions. Assumes the device is already formatted
/// as a swap device (i.e. with `mkswap`).
pub fn swapon(shell: &mut SshShell, device: &str) -> Result<(), failure::Error> {
    shell.run(cmd!("sudo swapon {}", device)).map(|_| ())
}

/// Reboot and wait for the remote machine to come back up again.
pub fn reboot(shell: &mut SshShell) -> Result<(), failure::Error> {
    let _ = shell.run(cmd!("sudo reboot"));

    // If we try to reconnect immediately, the machine will not have gone down yet.
    std::thread::sleep(std::time::Duration::from_secs(10));

    // Attempt to reconnect.
    shell.reconnect()?;

    // Make sure it worked.
    shell.run(cmd!("whoami"))?;

    Ok(())
}
