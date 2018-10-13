//! Functionality specific to Centos, RHEL, Amazon Linux, and other related distros.

use crate::ssh::{SshCommand, SshShell};

/// Install the given .rpm packages via `rpm`. Requires `sudo` priveleges.
pub fn dpkg_install(shell: SshShell, pkg: &str) -> Result<(), failure::Error> {
    shell.run(cmd!("sudo rpm -ivh {}", pkg)).map(|_| ())
}

/// Install the given list of packages via `yum install`. Requires `sudo` priveleges.
pub fn yum_install(shell: SshShell, pkgs: &[&str]) -> Result<(), failure::Error> {
    shell
        .run(cmd!("sudo yum install -y {}", pkgs.join(" ")))
        .map(|_| ())
}
