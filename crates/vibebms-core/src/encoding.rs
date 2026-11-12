//! Text decoding for BMS files.
//!
//! BMS files are traditionally Shift_JIS (MS932); Korean charts are often
//! EUC-KR (CP949) and newer ones UTF-8. We pick UTF-8 when the bytes are valid
//! UTF-8, otherwise try Shift_JIS and fall back to EUC-KR when Shift_JIS
//! decoding produces errors but EUC-KR does not.

use encoding_rs::{EUC_KR, SHIFT_JIS, UTF_8};

pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    let (sjis, _, sjis_err) = SHIFT_JIS.decode(bytes);
    if !sjis_err {
        // Valid Shift_JIS, but EUC-KR text is frequently *also* valid Shift_JIS
        // (as half-width katakana soup). Prefer EUC-KR when it decodes cleanly
        // and yields Hangul.
        let (euc, _, euc_err) = EUC_KR.decode(bytes);
        if !euc_err && hangul_ratio(&euc) > 0.3 && kana_ratio(&sjis) > 0.3 {
            return euc.into_owned();
        }
        return sjis.into_owned();
    }
    let (euc, _, euc_err) = EUC_KR.decode(bytes);
    if !euc_err {
        return euc.into_owned();
    }
    let (s, _, _) = UTF_8.decode(bytes);
    if s.chars().filter(|c| *c == '\u{FFFD}').count()
        <= sjis.chars().filter(|c| *c == '\u{FFFD}').count()
    {
        s.into_owned()
    } else {
        sjis.into_owned()
    }
}

fn non_ascii(s: &str) -> usize {
    s.chars().filter(|c| !c.is_ascii()).count().max(1)
}

fn hangul_ratio(s: &str) -> f64 {
    let h = s
        .chars()
        .filter(|c| ('\u{AC00}'..='\u{D7A3}').contains(c))
        .count();
    h as f64 / non_ascii(s) as f64
}

fn kana_ratio(s: &str) -> f64 {
    // half-width katakana is the tell-tale sign of mis-decoded EUC-KR
    let k = s
        .chars()
        .filter(|c| ('\u{FF61}'..='\u{FF9F}').contains(c))
        .count();
    k as f64 / non_ascii(s) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_common_encodings() {
        assert_eq!(decode_text("#TITLE テスト".as_bytes()), "#TITLE テスト");
        let (sjis, _, _) = SHIFT_JIS.encode("#TITLE テスト曲");
        assert_eq!(decode_text(&sjis), "#TITLE テスト曲");
        let (euc, _, _) = EUC_KR.encode("#TITLE 테스트 곡 제목입니다");
        assert_eq!(decode_text(&euc), "#TITLE 테스트 곡 제목입니다");
    }
}
