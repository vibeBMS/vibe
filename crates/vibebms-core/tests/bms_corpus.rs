//! Every chart of a corpus parses to the same `Chart` as when
//! `data/bms_corpus.txt` was recorded (a digest of each chart's `Debug`
//! form). The corpus: the test bot's charts, the stored replay's chart and
//! charts made up here that use every header, channel and control flow
//! command the parser knows, with the odd spellings files have.
//! `GOLDEN_BLESS=1` records the file anew.

use std::fmt::Write as _;
use vibebms_core::bms::{parse_bms, ParseOptions};
use vibebms_core::bot;
use vibebms_core::rng::JavaRandom;

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

/// A made-up chart: headers, definitions, channel lines and `#RANDOM` /
/// `#SWITCH` blocks drawn from `seed`. Its extension.
fn made_up(seed: i64) -> (String, &'static str) {
    // (consecutive seeds draw alike first values)
    let mut rng = JavaRandom::new(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15_u64 as i64));
    let mut n = |bound: i32| rng.next_int_bounded(bound) as usize;
    let ext = ["bms", "bme", "pms", "bml"][n(4)];
    let b62 = n(4) == 0;
    let digits = if b62 {
        "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
    } else {
        "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    };
    let id = |rng: &mut dyn FnMut(i32) -> usize| {
        let d = digits.as_bytes();
        let a = d[rng(3)] as char;
        let b = d[rng(d.len() as i32)] as char;
        format!("{a}{b}")
    };
    let nl = if n(3) == 0 { "\r\n" } else { "\n" };
    let mut s = String::new();
    let line = |s: &mut String, l: String| {
        s.push_str(&l);
        s.push_str(nl);
    };
    // headers in random case, some twice, some unknown
    let headers = [
        "TITLE Song [X]",
        "subtitle -A-",
        "ARTIST someone",
        "SubArtist obj: a",
        "SUBARTIST obj: b",
        "GENRE TEST",
        "PLAYLEVEL 12",
        "DIFFICULTY 4",
        "RANK 1",
        "DEFEXRANK 120.5",
        "TOTAL 350",
        "STAGEFILE stage.png",
        "BANNER banner.png",
        "BACKBMP back.png",
        "PREVIEW preview.ogg",
        "LNTYPE 1",
        "LNMODE 2",
        "lnmode 3",
        "PLAYER 1",
        "COMMENT hello",
        "%URL x",
        "BPM 150",
        "bpm 133.5 ;comment",
        "BPM 0",
    ];
    if b62 && n(2) == 0 {
        line(&mut s, "#BASE 62".into());
    }
    for _ in 0..8 + n(12) {
        line(&mut s, format!("#{}", headers[n(headers.len() as i32)]));
    }
    let mut defined = vec!["01".to_string()];
    for _ in 0..4 + n(30) {
        let i = id(&mut n);
        let what = ["WAV", "wav", "BMP", "BPM", "EXBPM", "STOP", "SCROLL", "Wav"][n(8)];
        let arg = match what {
            "BMP" => format!("img{}.bmp", n(9)),
            "BPM" | "EXBPM" => format!("{}", 60 + n(200)),
            "STOP" => format!("{}", n(200)),
            "SCROLL" => ["0.5", "2", "-1", "0"][n(4)].to_string(),
            _ => format!("s{}.wav", n(99)),
        };
        line(&mut s, format!("#{what}{i} {arg}"));
        defined.push(i);
    }
    if n(3) == 0 {
        line(
            &mut s,
            format!("#LNOBJ {}", defined[n(defined.len() as i32)]),
        );
    }
    if b62 && n(2) == 0 {
        line(&mut s, "#BASE 62".into());
    }
    let channels = [
        "01", "01", "02", "03", "08", "09", "SC", "sc", "04", "06", "07", "0A", "11", "12", "13",
        "14", "15", "16", "17", "18", "19", "21", "22", "23", "24", "25", "26", "28", "29", "31",
        "36", "41", "51", "52", "56", "58", "59", "61", "66", "D1", "d6", "E2", "11", "12", "16",
        "1a", "99",
    ];
    // (depth of open blocks, kind) for #RANDOM / #SWITCH nesting
    let mut open: Vec<bool> = Vec::new();
    for m in 0..10 + n(30) {
        match n(12) {
            0 => {
                line(&mut s, format!("#RANDOM {}", 1 + n(4)));
                line(&mut s, format!("#IF {}", 1 + n(3)));
                open.push(true);
            }
            1 if open.last() == Some(&true) => {
                let next = ["#ELSEIF 2", "#ELSE", "#IF 1", "#ENDIF"][n(4)];
                line(&mut s, next.into());
            }
            2 if open.last() == Some(&true) => {
                line(&mut s, "#ENDIF".into());
                if n(2) == 0 {
                    line(&mut s, "#ENDRANDOM".into());
                }
                open.pop();
            }
            3 => {
                line(&mut s, format!("#SWITCH {}", 1 + n(3)));
                line(&mut s, format!("#CASE {}", 1 + n(3)));
                open.push(false);
            }
            4 if open.last() == Some(&false) => {
                let next = ["#SKIP", "#DEF", "#CASE 2", "#CASE 1"][n(4)];
                line(&mut s, next.into());
            }
            5 if open.last() == Some(&false) => {
                line(&mut s, "#ENDSW".into());
                open.pop();
            }
            6 => line(&mut s, format!("#SETRANDOM {}", 1 + n(3))),
            _ => {}
        }
        for _ in 0..1 + n(8) {
            let ch = channels[n(channels.len() as i32)];
            let data = match ch {
                "02" => ["0.75", "1.5", "0.5", "1", "0", "-2"][n(6)].to_string(),
                _ => {
                    let slots = [1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48][n(11)];
                    let mut d = String::new();
                    for k in 0..slots {
                        if n(3) == 0 {
                            d.push_str("00");
                        } else if ch == "03" {
                            d.push_str(&format!("{:02X}", 40 + n(200)));
                        } else if n(4) == 0 {
                            d.push_str(&id(&mut n));
                        } else {
                            d.push_str(&defined[n(defined.len() as i32)]);
                        }
                        if k % 4 == 3 && n(6) == 0 {
                            d.push(' ');
                        }
                    }
                    if n(20) == 0 {
                        d.push('0');
                    }
                    d
                }
            };
            let lead = if n(10) == 0 { "  " } else { "" };
            line(&mut s, format!("{lead}#{m:03}{ch}:{data}"));
        }
    }
    (s, ext)
}

fn corpus() -> Vec<(String, Vec<u8>, &'static str, ParseOptions)> {
    let seeded = |seed| ParseOptions {
        random_seed: seed,
        random_selections: None,
    };
    let mut v = Vec::new();
    for keys in [5, 7, 10, 14] {
        for seed in 0..4 {
            let text = bot::chart_text(keys, seed);
            v.push((
                format!("bot {keys}K {seed}"),
                text.into_bytes(),
                "bme",
                seeded(0),
            ));
        }
    }
    let replay = include_bytes!("data/replay/chart.bme").to_vec();
    v.push(("replay chart".into(), replay, "bme", seeded(0)));
    for seed in 0..400 {
        let (text, ext) = made_up(seed);
        let opts = match seed % 4 {
            3 => ParseOptions {
                random_seed: seed,
                random_selections: Some(vec![2, 1, 3]),
            },
            _ => seeded(seed * 7919),
        };
        v.push((
            format!("made up {seed}.{ext}"),
            text.into_bytes(),
            ext,
            opts,
        ));
    }
    v
}

#[test]
fn the_corpus_parses_as_recorded() {
    let mut got = String::new();
    for (name, bytes, ext, opts) in corpus() {
        let c = parse_bms(&bytes, ext, &opts).unwrap();
        let digest = fnv(&format!("{c:?}"));
        let _ = writeln!(got, "{name}: {} notes, {digest:016x}", c.notes.len());
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bms_corpus.txt");
    if std::env::var_os("GOLDEN_BLESS").is_some() {
        std::fs::write(path, &got).unwrap();
    }
    let want = std::fs::read_to_string(path).unwrap();
    for (g, w) in got.lines().zip(want.lines()) {
        assert_eq!(g, w);
    }
    assert_eq!(got.lines().count(), want.lines().count());
}
