# bmseeker

BMS 라이브러리 도구: 난이도표를 읽고, 라이브러리에 없는 곡을 작가·난이도표가 걸어 둔
링크에서 바로 받아 풀고, 차분(差分) 채보를 그 곡의 폴더에 넣습니다.
vibeBMS가 쓰는 라이브러리이자, 게임 없이 쓰는 명령줄 도구입니다.

```text
bmseeker table <난이도표 URL>                        레벨별 채보 수
bmseeker missing <난이도표 URL> <라이브러리> [-l 레벨] [--get]
                                                     라이브러리에 없는 곡 (--get: 받기)
bmseeker fetch <링크>... <파일 또는 폴더> [-t 제목] [-a 아티스트] [--md5 MD5] [--search] [--any] [--json]
                                                     링크를 차례로 시도해 아카이브 받기 (폴더면 원래 파일 이름으로;
                                                     --search / --md5: 다 실패하면 BMS SEARCH의 링크도;
                                                     --any: 페이지가 아닌 단일 파일(채보, 동영상)도; --json: 결과를 JSON 한 줄로)
bmseeker lookup [-t 제목] [-a 아티스트] [--md5 MD5]       BMS SEARCH에 있는 그 곡의 다운로드 링크
bmseeker install <링크>... <라이브러리> [--diff] [-t 제목] [-a 아티스트] [--md5 MD5]
                                                     곡(또는 차분)을 라이브러리에 (링크를 차례로)
bmseeker import <파일> <라이브러리>                   받아 둔 파일을 라이브러리에
bmseeker place <채보> <라이브러리> [--to 폴더] [--min 퍼센트]
                                                     차분이 들어갈 곡 폴더 후보 (또는 옮기기)
bmseeker pending <라이브러리>                         곡을 기다리는 차분
bmseeker manbow <이벤트 번호> <폴더>                   manbow 이벤트의 작품과 링크, 어느 링크가 본체/애드온/미러인지를
                                                     <폴더>/works.json으로 (<폴더>/overrides.json: 손으로 정한 것)
bmseeker check-links <works.json> <links.json> [--days N] [--minutes M]
                                                     작품 링크마다 받아 보고 무엇인지 기록 (본체 / 애드온 / 미디어, 죽은 링크)
bmseeker export lr2 <난이도표 URL> <폴더>             LR2 커스텀 폴더 (그 폴더를 LR2에 추가)
bmseeker export bmt <난이도표 URL> <beatoraja table 폴더>
                                                     beatoraja 난이도표 캐시
```

`missing`과 `export`는 채보의 다른 링크를 MD5로 찾습니다: 링크가 비었거나 죽었으면 채우고, 받아지지
않으면(페이지, OneDrive 등) 다음 링크를 차례로 시도합니다. 찾는 곳은 그 난이도표, `--also <URL>`로 준
다른 난이도표, Stella 업로더 목록, `--links <파일 또는 URL>`로 준 TSV(`md5`, `url_diff`, `url`), 그리고 곡을
받을 때 마지막으로 [BMS SEARCH](https://bmssearch.net) API(채보 → 작품 → 작가가 올린 다운로드 링크, 작품에 묶이지 않은
차분은 제목·아티스트로 작품 검색)입니다. `--own-links`: 난이도표의 링크만.

`cargo run -p bmseeker -- missing https://stellabms.xyz/sl/table.html ~/bms -l 0`

## 창 (`bmseeker-gui`)

`cargo run --release -p bmseeker-gui`: 같은 일을 창으로 합니다(egui).

- **Tables**: 난이도표·레벨별로 가진 채보 수, 레벨의 채보 목록(가짐/없음, 곡·차분 링크), 하나씩 또는
  레벨의 없는 곡 전부 받기. 받은 뒤 라이브러리를 다시 읽어 바로 반영합니다.
- **Waiting**: 곡보다 먼저 받은 차분과 들어갈 수 있는 폴더 후보(음원 충족률, 같은 곡 여부), 골라서 옮기기.
- **Settings**: BMS 폴더, 난이도표 목록, MD5로 다른 링크 찾기(등록한 난이도표들·Stella 업로더), LR2 커스텀 폴더·beatoraja table 폴더로 내보내기.

설정과 라이브러리 해시 캐시는 사용자 설정 폴더의 `bmseeker/`(Windows `%APPDATA%`, macOS
`~/Library/Application Support`, 그 밖 `~/.config`)에 둡니다. 받은 파일은 BMS 폴더의 `.bmseeker/`에서
곡을 기다립니다(명령줄 도구와 같은 곳).

## 모듈

| 모듈 | 하는 일 |
| --- | --- |
| `table` | bmstable 형식 난이도표 (HTML의 `<meta name="bmstable">` → header → data) |
| `hosts` | 링크 → 아카이브: 직링크, Google Drive 파일·폴더, Dropbox, MediaFire, MEGA 파일·폴더(합본 안에서 제목·아티스트로 찾기), manbow·BMS SEARCH(venue.bmssearch.net) 이벤트 작품 페이지(manbow 작품은 [이벤트 미러](https://github.com/vibeBMS/BMS)에 있으면 미러에서). OneDrive는 브라우저에서 `PRESS_DOWNLOAD_JS`로 |
| `unpack` / `import` | .zip / .7z / .rar 풀기 (Shift_JIS·EUC-KR 이름, 경로 탈출·압축 폭탄 방지) |
| `placement` | 차분 채보가 어느 곡 폴더에 속하는지: 함께 온 파일까지 쳐서 쓰는 음원의 99% 이상 + 같은 곡(제목·아티스트), 아니면 후보 3개; 같은 이름 파일은 스마트 덮어쓰기 |
| `install` | 난이도표의 곡 하나: 본체 링크와 차분 링크를 받아 라이브러리에 |
| `links` | 채보 MD5로 다른 링크 찾기 (등록한 난이도표들, Stella 업로더 목록): 빈·죽은 링크 채우기, 안 받아지면 차례로 시도 |
| `manbow` | manbow 이벤트: URL 목록·작품 목록에서 작품과 링크(라벨), 어느 링크가 본체(HQ > ogg > 파일, 최신 패키지)·애드온(추가 채보, BGA 단독)·미러인지 (BMS 이벤트 미러의 `works.json`, 작품 페이지에서 받을 링크) |
| `bmssearch` | BMS SEARCH API로 채보의 작품과 그 다운로드 링크 찾기 (그래도 안 받아질 때) |
| `probe` | 링크를 받아 풀어 보고 분류: 받아짐/페이지/없음(404·410)/오류, 본체(채보 음원의 90% 이상이 안에)·애드온(차분)·미디어(채보 없음) |
| `export` | 난이도표를 LR2 커스텀 폴더(`.lr2folder`)나 beatoraja 캐시(`.bmt`)로 |
| `fs` | 파일 접근 (`Disk`, 또는 플레이어가 라이브러리를 두는 곳) |

네트워크 부분은 IO 없는 단계 기계(`hosts::Resolver`, `table::TableLoader`)라, 블로킹으로도
(네이티브의 `hosts::fetch`) 브라우저 비동기로도(vibeBMS 웹 빌드) 같은 로직을 돌립니다.

## 출처

설치 폴더 추정·스마트 덮어쓰기·링크 보완·내보내기의 규칙은
[BeMusicSeeker](https://github.com/Neeted/bemusicseeker-unofficial-fork)(rib_2_bit 원작, Neeted 비공식 포크, MIT)의
동작을 따라 Rust로 새로 구현했습니다.

## 라이선스

`MIT OR Apache-2.0`. `.rar`는 [unrar-rs](https://github.com/scryer-media/rarpar/tree/main/crates/unrar-rs)
(Apache-2.0, UnRAR 라이선스 제한 포함)로 풉니다.

> UnRAR source code may be used in any software to handle RAR archives without limitations free of charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR compression algorithm, which is proprietary. Distribution of modified UnRAR source code in separate form or as a part of other software is permitted, provided that full text of this paragraph, starting from "UnRAR source code" words, is included in license, or in documentation if license is not available, and in source code comments of resulting package.

웹(wasm)에서 쓰려면 워크스페이스의 `[patch.crates-io] unrar-rs` 패치가 필요합니다
(`third_party/unrar-rs/VIBEBMS.md`).
