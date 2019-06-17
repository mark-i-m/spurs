//! Functionality specific to Ubuntu.

use spurs::{cmd, ssh::SshCommand};

/// Install the given .deb packages via `dpkg`. Requires `sudo` priveleges.
pub fn dpkg_install(pkg: &str) -> SshCommand {
    cmd!("sudo dpkg -i {}", pkg)
}

/// Install the given list of packages via `apt-get install`. Requires `sudo` priveleges.
pub fn apt_install(pkgs: &[&str]) -> SshCommand {
    cmd!("sudo apt-get -y install {}", pkgs.join(" "))
}

#[cfg(test)]
mod test {
    use spurs::ssh::SshCommand;

    #[test]
    fn test_dpkg_install() {
        assert_eq!(
            super::dpkg_install("foobar"),
            SshCommand::make_cmd(
                "sudo dpkg -i foobar".into(),
                None,
                false,
                false,
                false,
                false,
            ),
        );
    }

    #[test]
    fn test_apt_install() {
        assert_eq!(
            super::apt_install(&["foobar"]),
            SshCommand::make_cmd(
                "sudo apt-get -y install foobar".into(),
                None,
                false,
                false,
                false,
                false,
            ),
        );
    }
}
