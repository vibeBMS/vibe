# bmseeker

BMS 라이브러리 도구: 난이도표를 읽고, 라이브러리에 없는 곡을 작가·난이도표가 걸어 둔
링크에서 바로 받아 풀고, 차분(差分) 채보를 그 곡의 폴더에 넣습니다.
vibeBMS가 쓰는 라이브러리이자, 게임 없이 쓰는 명령줄 도구입니다.

```text
bmseeker table <난이도표 URL>                        레벨별 채보 수
bmseeker missing <난이도표 URL> <라이브러리> [-l 레벨] [--get]
                                                     라이브러리에 없는 곡 (--get: 받기)
bmseeker fetch <링크> <파일> [-t 제목] [-a 아티스트]   링크가 가리키는 아카이브 받기
bmseeker install <링크> <라이브러리> [--diff] [-t 제목] [-a 아티스트]
                                                     곡(또는 차분)을 라이브러리에
bmseeker import <파일> <라이브러리>                   받아 둔 파일을 라이브러리에
```

`cargo run -p bmseeker -- missing https://stellabms.xyz/sl/table.html ~/bms -l 0`

## 모듈

| 모듈 | 하는 일 |
| --- | --- |
| `table` | bmstable 형식 난이도표 (HTML의 `<meta name="bmstable">` → header → data) |
| `hosts` | 링크 → 아카이브: 직링크, Google Drive 파일·폴더, Dropbox, MediaFire, MEGA 파일·폴더(합본 안에서 제목·아티스트로 찾기), manbow 이벤트 작품 페이지 |
| `unpack` / `import` | .zip / .7z / .rar 풀기 (Shift_JIS·EUC-KR 이름, 경로 탈출·압축 폭탄 방지) |
| `placement` | 차분 채보가 어느 곡 폴더에 속하는지 (쓰는 음원 + 제목·아티스트) |
| `install` | 난이도표의 곡 하나: 본체 링크와 차분 링크를 받아 라이브러리에 |
| `fs` | 파일 접근 (`Disk`, 또는 플레이어가 라이브러리를 두는 곳) |

네트워크 부분은 IO 없는 단계 기계(`hosts::Resolver`, `table::TableLoader`)라, 블로킹으로도
(네이티브의 `hosts::fetch`) 브라우저 비동기로도(vibeBMS 웹 빌드) 같은 로직을 돌립니다.

## 라이선스

`MIT OR Apache-2.0`. `.rar`는 [unrar-rs](https://github.com/scryer-media/rarpar/tree/main/crates/unrar-rs)
(Apache-2.0, UnRAR 라이선스 제한 포함)로 풉니다.

> UnRAR source code may be used in any software to handle RAR archives without limitations free of charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR compression algorithm, which is proprietary. Distribution of modified UnRAR source code in separate form or as a part of other software is permitted, provided that full text of this paragraph, starting from "UnRAR source code" words, is included in license, or in documentation if license is not available, and in source code comments of resulting package.

웹(wasm)에서 쓰려면 워크스페이스의 `[patch.crates-io] unrar-rs` 패치가 필요합니다
(`third_party/unrar-rs/VIBEBMS.md`).
