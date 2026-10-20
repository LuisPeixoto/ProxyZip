//! Configuration profile that installs the ProxyZip CA on a physical iPhone or iPad.

use base64::Engine;
use md5::{Digest, Md5};

/// A stable UUID derived from `seed`, so reinstalling the profile replaces the old one.
fn uuid_from(seed: &[u8], salt: &str) -> String {
    let mut h = Md5::new();
    h.update(salt.as_bytes());
    h.update(seed);
    let d = h.finalize();
    let x = |r: std::ops::Range<usize>| d[r].iter().map(|b| format!("{b:02X}")).collect::<String>();
    format!("{}-{}-{}-{}-{}", x(0..4), x(4..6), x(6..8), x(8..10), x(10..16))
}

/// An Apple `.mobileconfig` file with the certificate as a root CA payload.
pub fn mobileconfig(cert_der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(cert_der);
    let payload_uuid = uuid_from(cert_der, "payload");
    let profile_uuid = uuid_from(cert_der, "profile");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <array>
    <dict>
      <key>PayloadCertificateFileName</key><string>proxyzip-ca.cer</string>
      <key>PayloadContent</key><data>{b64}</data>
      <key>PayloadDescription</key><string>Adds the ProxyZip root certificate</string>
      <key>PayloadDisplayName</key><string>ProxyZip CA</string>
      <key>PayloadIdentifier</key><string>dev.proxyzip.ca.root</string>
      <key>PayloadType</key><string>com.apple.security.root</string>
      <key>PayloadUUID</key><string>{payload_uuid}</string>
      <key>PayloadVersion</key><integer>1</integer>
    </dict>
  </array>
  <key>PayloadDescription</key><string>Lets ProxyZip inspect HTTPS traffic from this device. Remove it when you are done.</string>
  <key>PayloadDisplayName</key><string>ProxyZip CA</string>
  <key>PayloadIdentifier</key><string>dev.proxyzip.ca</string>
  <key>PayloadOrganization</key><string>ProxyZip</string>
  <key>PayloadRemovalDisallowed</key><false/>
  <key>PayloadType</key><string>Configuration</string>
  <key>PayloadUUID</key><string>{profile_uuid}</string>
  <key>PayloadVersion</key><integer>1</integer>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_embeds_the_certificate_and_is_stable() {
        let a = mobileconfig(b"certificate bytes");
        assert!(a.contains("com.apple.security.root"));
        assert!(a.contains(&base64::engine::general_purpose::STANDARD.encode(b"certificate bytes")));
        assert_eq!(a, mobileconfig(b"certificate bytes"), "same CA, same profile identity");
        assert_ne!(a, mobileconfig(b"another"));
    }

    #[test]
    fn uuids_have_the_canonical_shape() {
        let u = uuid_from(b"x", "p");
        let parts: Vec<_> = u.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn profile_is_a_valid_plist() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("p.mobileconfig");
        std::fs::write(&f, mobileconfig(b"abc")).unwrap();
        let out = std::process::Command::new("plutil").arg("-lint").arg(&f).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    }
}
