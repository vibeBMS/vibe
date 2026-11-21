//! `latest.json.sig`: the OpenSSH signature of `latest.json` that CI makes
//! with `ssh-keygen -Y sign -n vibebms-update` (the format is OpenSSH's
//! PROTOCOL.sshsig). Only an Ed25519 key's, over a SHA-512 of the file, is
//! read; everything else is refused.

use base64::{engine::general_purpose::STANDARD, Engine};

/// What the signature must have been made for (`ssh-keygen -n`): the key's
/// signature of anything else is not one of the update list.
pub const NAMESPACE: &str = "vibebms-update";

/// The largest signature file read (an Ed25519 one is about 300 bytes).
pub const MAX: usize = 4096;

const MAGIC: &[u8] = b"SSHSIG";
const ED25519: &[u8] = b"ssh-ed25519";
const HASH: &[u8] = b"sha512";

/// Whether `file` (the armoured signature) is `key`'s signature of `msg`;
/// why not when it is not.
pub fn verify(file: &[u8], msg: &[u8], key: &[u8; 32]) -> Result<(), String> {
    let blob = unarmour(file)?;
    let mut r = Reader(&blob);
    if r.take(MAGIC.len())? != MAGIC || r.u32()? != 1 {
        return Err("it is not an SSH signature of version 1".into());
    }
    let signer = r.ed25519(32)?;
    let namespace = r.string()?;
    let reserved = r.string()?;
    let hash = r.string()?;
    let signature = r.ed25519(64)?;
    if !r.0.is_empty() {
        return Err("it has more after the signature".into());
    }
    if namespace != NAMESPACE.as_bytes() {
        return Err(format!("it is not made for {NAMESPACE}"));
    }
    if hash != HASH {
        return Err("its hash is not SHA-512".into());
    }
    if signer != key {
        return Err("it is not by the update key".into());
    }
    // what was signed: the magic, then the namespace, the reserved field,
    // the hash's name and the hash of the message, each as a string
    let digest = ring::digest::digest(&ring::digest::SHA512, msg);
    let mut signed = MAGIC.to_vec();
    for s in [NAMESPACE.as_bytes(), reserved, HASH, digest.as_ref()] {
        signed.extend_from_slice(&(s.len() as u32).to_be_bytes());
        signed.extend_from_slice(s);
    }
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(&signed, signature)
        .map_err(|_| "it does not match the file".to_string())
}

/// The binary signature in the armour (`-----BEGIN SSH SIGNATURE-----`,
/// base64 lines, `-----END SSH SIGNATURE-----`), nothing around it.
fn unarmour(file: &[u8]) -> Result<Vec<u8>, String> {
    if file.len() > MAX {
        return Err(format!("it is larger than {MAX} bytes"));
    }
    let text = std::str::from_utf8(file).map_err(|_| "it is not text")?;
    let mut lines = text.lines();
    if lines.next() != Some("-----BEGIN SSH SIGNATURE-----") {
        return Err("it is not an armoured SSH signature".into());
    }
    let mut b64 = String::new();
    let mut end = false;
    for l in lines.by_ref() {
        if l == "-----END SSH SIGNATURE-----" {
            end = true;
            break;
        }
        b64.push_str(l);
    }
    if !end || lines.next().is_some() {
        return Err("it is not one armoured SSH signature".into());
    }
    STANDARD
        .decode(b64)
        .map_err(|_| "its signature is not base64".into())
}

/// The SSH wire format read from the front, each length checked against
/// what is left.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.0.len() {
            return Err("it is cut short".into());
        }
        let (a, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(a)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn string(&mut self) -> Result<&'a [u8], String> {
        let n = self.u32()?;
        self.take(n as usize)
    }

    /// A string holding `ssh-ed25519` and `len` bytes (a public key or a
    /// signature), nothing else.
    fn ed25519(&mut self, len: usize) -> Result<&'a [u8], String> {
        let mut inner = Reader(self.string()?);
        let (kind, bytes) = (inner.string()?, inner.string()?);
        if kind != ED25519 || bytes.len() != len || !inner.0.is_empty() {
            return Err("it is not an Ed25519 key's signature".into());
        }
        Ok(bytes)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    const MSG: &[u8] = include_bytes!("../../tests/data/update/latest.json");
    const GOOD: &[u8] = include_bytes!("../../tests/data/update/good.sig");

    /// The test key (tests/data/update/test.pub; its signatures were made
    /// by `ssh-keygen -Y sign`).
    pub fn test_key() -> [u8; 32] {
        super::super::key::parse(include_str!("../../tests/data/update/test.pub")).unwrap()
    }

    fn blob(file: &[u8]) -> Vec<u8> {
        unarmour(file).unwrap()
    }

    fn armour(blob: &[u8]) -> Vec<u8> {
        let b64 = STANDARD.encode(blob);
        let lines: Vec<_> = b64
            .as_bytes()
            .chunks(70)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        format!(
            "-----BEGIN SSH SIGNATURE-----\n{}\n-----END SSH SIGNATURE-----\n",
            lines.join("\n")
        )
        .into_bytes()
    }

    /// `blob` with the string at `at` (its length's offset) replaced by `s`.
    fn with_string(blob: &[u8], at: usize, s: &[u8]) -> Vec<u8> {
        let n = u32::from_be_bytes(blob[at..at + 4].try_into().unwrap()) as usize;
        let mut b = blob[..at].to_vec();
        b.extend_from_slice(&(s.len() as u32).to_be_bytes());
        b.extend_from_slice(s);
        b.extend_from_slice(&blob[at + 4 + n..]);
        b
    }

    #[test]
    fn openssh_signatures_are_checked() {
        let key = test_key();
        verify(GOOD, MSG, &key).unwrap();
        // the armour as written again (other line lengths, Windows line ends)
        verify(&armour(&blob(GOOD)), MSG, &key).unwrap();
        let crlf = String::from_utf8(GOOD.to_vec())
            .unwrap()
            .replace('\n', "\r\n");
        verify(crlf.as_bytes(), MSG, &key).unwrap();
        // any byte of the file changed
        for i in [0, MSG.len() / 2, MSG.len() - 1] {
            let mut m = MSG.to_vec();
            m[i] ^= 1;
            assert_eq!(
                verify(GOOD, &m, &key),
                Err("it does not match the file".into())
            );
        }
        let e = |file: &[u8]| verify(file, MSG, &key).unwrap_err();
        // made for another namespace, by another key, over SHA-256
        let other = include_bytes!("../../tests/data/update/namespace.sig");
        assert!(e(other).contains("not made for"), "{}", e(other));
        let other = include_bytes!("../../tests/data/update/other-key.sig");
        assert!(e(other).contains("update key"), "{}", e(other));
        let other = include_bytes!("../../tests/data/update/sha256.sig");
        assert!(e(other).contains("SHA-512"), "{}", e(other));
        // another key given
        let mut k = key;
        k[0] ^= 1;
        assert!(verify(GOOD, MSG, &k).is_err());
    }

    #[test]
    fn broken_signatures_are_refused() {
        let key = test_key();
        let b = blob(GOOD);
        let e = |file: &[u8]| verify(file, MSG, &key).unwrap_err();
        // cut short at every length, of the file (but its last line end)
        // and of its blob
        for n in 0..GOOD.len() - 1 {
            assert!(verify(&GOOD[..n], MSG, &key).is_err(), "{n}");
        }
        for n in 0..b.len() {
            assert!(verify(&armour(&b[..n]), MSG, &key).is_err(), "{n}");
        }
        // more after it: in the blob, after the armour, a second one
        assert!(e(&armour(&[&b[..], b"\0"].concat())).contains("more after"));
        assert!(verify(&[GOOD, b"\n"].concat(), MSG, &key).is_err());
        assert!(verify(&[GOOD, b"x"].concat(), MSG, &key).is_err());
        assert!(verify(&[GOOD, GOOD].concat(), MSG, &key).is_err());
        // its magic and version
        let mut m = b.clone();
        m[0] = b'X';
        assert!(e(&armour(&m)).contains("version 1"));
        let mut m = b.clone();
        m[9] = 2;
        assert!(e(&armour(&m)).contains("version 1"));
        // (the blob: magic 6, version 4, then the public key's string)
        let key_at = 10;
        let pk_len = 4 + 4 + ED25519.len() + 4 + 32;
        let ns_at = key_at + pk_len;
        assert_eq!(
            &b[ns_at + 4..ns_at + 4 + NAMESPACE.len()],
            NAMESPACE.as_bytes()
        );
        // a key of another kind, or of another size
        let pk = &b[key_at + 4..key_at + pk_len];
        let rsa = [&b"\0\0\0\x07ssh-rsa"[..], &pk[15..]].concat();
        assert!(e(&armour(&with_string(&b, key_at, &rsa))).contains("Ed25519"));
        let short = [&pk[..15], &[0, 0, 0, 31], &pk[19..50]].concat();
        assert!(e(&armour(&with_string(&b, key_at, &short))).contains("Ed25519"));
        let long = [pk, &[0][..]].concat();
        assert!(e(&armour(&with_string(&b, key_at, &long))).contains("Ed25519"));
        // the namespace changed after signing: it is refused, and a hash
        // named but not allowed is too
        let other = with_string(&b, ns_at, b"vibebms-updatf");
        assert!(e(&armour(&other)).contains("not made for"));
        let hash_at = ns_at + 4 + NAMESPACE.len() + 4;
        assert_eq!(&b[hash_at + 4..hash_at + 10], HASH);
        for h in [&b"sha256"[..], b"sha1", b"", b"SHA512"] {
            assert!(e(&armour(&with_string(&b, hash_at, h))).contains("SHA-512"));
        }
        // the reserved field is signed
        let reserved_at = ns_at + 4 + NAMESPACE.len();
        let r = with_string(&b, reserved_at, b"x");
        assert_eq!(e(&armour(&r)), "it does not match the file");
        // lengths that run past the end, at each length field
        for at in [
            10,
            14,
            29,
            ns_at,
            reserved_at,
            hash_at,
            hash_at + 10,
            hash_at + 14,
            hash_at + 29,
        ] {
            let mut h = b.clone();
            h[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
            assert!(verify(&armour(&h), MSG, &key).is_err(), "{at}");
            h[at..at + 4].copy_from_slice(&0x8000_0000u32.to_be_bytes());
            assert!(verify(&armour(&h), MSG, &key).is_err(), "{at}");
        }
        // not base64, not text, not armoured, empty, too large
        let text = String::from_utf8(GOOD.to_vec()).unwrap();
        let body = text.lines().nth(1).unwrap();
        assert!(e(text.replacen(body, &body.replace('A', "*"), 1).as_bytes()).contains("base64"));
        assert!(e(text.replacen(body, &format!(" {body}"), 1).as_bytes()).contains("base64"));
        assert!(e(&[0xff, 0xfe]).contains("not text"));
        assert!(e(b"SSHSIG").contains("armoured"));
        assert!(e(text.replace("SSH SIGNATURE", "PGP SIGNATURE").as_bytes()).contains("armoured"));
        assert!(
            e(text.replace("-----END SSH SIGNATURE-----", "").as_bytes()).contains("one armoured")
        );
        assert!(e(b"").contains("armoured"));
        let big = armour(&[&b[..], &vec![0; MAX]].concat());
        assert!(e(&big).contains("larger"));
    }
}
