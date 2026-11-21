//! The update key's file, `tools/update_key.pub`: build.rs reads it into the
//! build ([`super::KEY`]) and fails when it is not one Ed25519 key.

use base64::{engine::general_purpose::STANDARD, Engine};

/// The key of an OpenSSH public key file of one line,
/// `ssh-ed25519 <base64> [comment]`.
pub fn parse(text: &str) -> Result<[u8; 32], String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let (Some(line), None) = (lines.next(), lines.next()) else {
        return Err("it is not one line `ssh-ed25519 AAAA… <comment>`".into());
    };
    let mut fields = line.split_ascii_whitespace();
    if fields.next() != Some("ssh-ed25519") {
        return Err("it is not an ssh-ed25519 key".into());
    }
    let blob = fields
        .next()
        .and_then(|b| STANDARD.decode(b).ok())
        .ok_or("its key is not base64")?;
    // string "ssh-ed25519", string <the 32 bytes>
    blob.strip_prefix(b"\0\0\0\x0bssh-ed25519\0\0\0\x20")
        .and_then(|k| <[u8; 32]>::try_from(k).ok())
        .ok_or_else(|| "it is not an Ed25519 public key".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_files() {
        let line = include_str!("../../tests/data/update/test.pub");
        let key = parse(line).unwrap();
        // (the blob's last 32 bytes)
        let blob = STANDARD.decode(line.split(' ').nth(1).unwrap()).unwrap();
        assert_eq!(key[..], blob[19..]);
        // without its comment, with Windows line ends and blank lines
        let bare = line.rsplit_once(' ').unwrap().0;
        assert_eq!(parse(bare), Ok(key));
        assert_eq!(parse(&format!("\r\n{bare}\r\n\r\n")), Ok(key));
        let other = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICO8WVP6tj130Wwb1aPvUIQubPw7pMntUBT+V5Dd38LR other";
        assert_ne!(parse(other).unwrap(), key);
        let b64 = |b: &[u8]| STANDARD.encode(b);
        for bad in [
            String::new(),
            "\n\n".into(),
            format!("{line}{line}"),
            line.replacen("ssh-ed25519", "ssh-rsa", 1),
            "ssh-ed25519".into(),
            "ssh-ed25519 !!!!".into(),
            // a key of another kind under the name, one cut short, one too long
            format!("ssh-ed25519 {}", b64(&[&blob[..4], b"ssh-ed25518", &blob[15..]].concat())),
            format!("ssh-ed25519 {}", b64(&blob[..50])),
            format!("ssh-ed25519 {}", b64(&[&blob[..], &[0]].concat())),
            // the private key's file
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\n".into(),
        ] {
            assert!(parse(&bad).is_err(), "{bad:?}");
        }
    }
}
