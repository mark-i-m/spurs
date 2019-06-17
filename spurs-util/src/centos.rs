//! Functionality specific to Centos, RHEL, Amazon Linux, and other related distros.

use spurs::{cmd, ssh::SshCommand};

/// Install the given .rpm packages via `rpm`. Requires `sudo` priveleges.
pub fn rpm_install(pkg: &str) -> SshCommand {
    cmd!("sudo rpm -ivh {}", pkg)
}

/// Install the given list of packages via `yum install`. Requires `sudo` priveleges.
pub fn yum_install(pkgs: &[&str]) -> SshCommand {
    cmd!("sudo yum install -y {}", pkgs.join(" "))
}

#[cfg(test)]
mod test {
    use spurs::ssh::SshCommand;

    #[test]
    fn test_rpm_install() {
        assert_eq!(
            super::rpm_install("foobar"),
            SshCommand::make_cmd(
                "sudo rpm -ivh foobar".into(),
                None,
                false,
                false,
                false,
                false,
            ),
        );
    }

    #[test]
    fn test_yum_install() {
        assert_eq!(
            super::yum_install(&["foobar"]),
            SshCommand::make_cmd(
                "sudo yum install -y foobar".into(),
                None,
                false,
                false,
                false,
                false,
            ),
        );
    }
}
