#![forbid(unsafe_code)]

/// Returns the version assigned to the SaniKey workspace packages.
///
/// # Examples
///
/// ```
/// assert_eq!(sanikey_core::version(), "0.1.0");
/// ```
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_matches_workspace_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
