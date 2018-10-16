//! Functionality specific to Ubuntu.

use crate::ssh::SshCommand;

/// Install the given .deb packages via `dpkg`. Requires `sudo` priveleges.
pub fn dpkg_install(pkg: &str) -> SshCommand {
    cmd!("sudo dpkg -i {}", pkg)
}

/// Install the given list of packages via `apt-get install`. Requires `sudo` priveleges.
pub fn apt_install(pkgs: &[&str]) -> SshCommand {
    cmd!("sudo apt-get -y install {}", pkgs.join(" "))
}
