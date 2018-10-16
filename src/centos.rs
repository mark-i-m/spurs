//! Functionality specific to Centos, RHEL, Amazon Linux, and other related distros.

use crate::ssh::SshCommand;

/// Install the given .rpm packages via `rpm`. Requires `sudo` priveleges.
pub fn dpkg_install(pkg: &str) -> SshCommand {
    cmd!("sudo rpm -ivh {}", pkg)
}

/// Install the given list of packages via `yum install`. Requires `sudo` priveleges.
pub fn yum_install(pkgs: &[&str]) -> SshCommand {
    cmd!("sudo yum install -y {}", pkgs.join(" "))
}
