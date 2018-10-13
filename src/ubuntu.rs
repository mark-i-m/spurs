//! Functionality specific to Ubuntu.

use crate::ssh::{SshCommand, SshShell};

/// Install the given .deb packages via `dpkg`. Requires `sudo` priveleges.
pub fn dpkg_install(shell: SshShell, pkg: &str) -> Result<(), failure::Error> {
    shell.run(cmd!("sudo dpkg -i {}", pkg)).map(|_| ())
}

/// Install the given list of packages via `apt-get install`. Requires `sudo` priveleges.
pub fn apt_install(shell: SshShell, pkgs: &[&str]) -> Result<(), failure::Error> {
    shell
        .run(cmd!("sudo apt-get -y install {}", pkgs.join(" ")))
        .map(|_| ())
}
