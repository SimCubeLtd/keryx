//! Share a draft version as a single-layer OCI artifact. This is the only
//! crate that depends on `oci-client` and `docker_credential`.

#[cfg(test)]
mod tests {
    /// Proves oci-client compiles and links with no TLS feature of its own,
    /// on the ring provider the binary installs.
    #[test]
    fn oci_client_builds_on_the_ring_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = oci_client::Client::try_from(oci_client::client::ClientConfig::default());
        assert!(
            client.is_ok(),
            "oci-client failed to build its HTTPS client"
        );
    }
}
