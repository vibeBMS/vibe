# bmseeker

BMS 라이브러리 도구: 난이도표를 읽고, 라이브러리에 없는 곡을 작가·난이도표가 걸어 둔
링크에서 바로 받아 풀고, 차분(差分) 채보를 그 곡의 폴더에 넣습니다.
vibeBMS가 쓰는 라이브러리이자, 게임 없이 쓰는 명령줄 도구입니다.

```text
bmseeker [--json] [--config 파일] <명사> <동사> ...

link resolve <링크>... [-t 제목 -a 아티스트]           링크가 가리키는 것 (파일 URL·이름 / 브라우저가 필요한 페이지 / 오류), 받지 않음
link get <링크>... [-o 폴더|파일] [-t -a --md5] [--search] [--any]
                                                     링크를 차례로 시도해 받기 (폴더면 원래 파일 이름으로;
                                                     --search: 다 실패하면 소스들(BMS SEARCH...)의 링크도;
                                                     --any: 페이지가 아닌 단일 파일(채보, 동영상)도)
song find [-t -a --md5] [--diff] [--table URL]... [--list 파일|URL]...
                                                     소스들이 아는 그 곡(--diff: 차분)의 링크
table show <URL>                                     난이도표와 레벨별 채보 수
table missing <URL> -L 라이브러리 [-l 레벨] [--get] [--also URL]... [--list ...] [--own-links]
                                                     라이브러리에 없는 채보 (--get: 받기)
table export lr2|bmt <URL> <폴더>                     LR2 커스텀 폴더 / beatoraja 난이도표 캐시
library import <파일> -L 라이브러리                   받아 둔 파일을 라이브러리에
library place <채보> -L 라이브러리 [--to 폴더] [--min 퍼센트]
                                                     차분이 들어갈 곡 폴더 후보 (또는 옮기기)
library pending -L 라이브러리                          곡을 기다리는 차분
library install <링크>... -L 라이브러리 [--diff] [-t -a --md5] [--own-links]
                                                     곡(또는 차분)을: 링크를 차례로, 그다음 소스들의 링크
event works <manbow 이벤트 번호> [--overrides 파일|-]  작품마다 링크와 어느 것이 본체/애드온/미러인지 (JSON Lines)
event check [작품 파일|-] [--known 파일] [--days N] [--minutes M]
                                                     작품 링크마다 받아 보고 무엇인지 (JSON Lines)
```

`--json`이면 모든 명령이 JSON Lines(한 줄에 레코드 하나, 나오는 대로)를 씁니다. `event` 명령은 늘 그렇고,
레코드를 파일이나 `-`(stdin)에서 읽습니다 (JSON Lines, 또는 JSON 배열, 또는 키별 객체). 사람을 위한 메모는
stderr로만 나갑니다.

```text
work   {"num","title","artist","team","kind","size_kb","page","links":[{"label","url"}],"main":[url],"extra":[[url]],"review"?, ...}
check  {"url","state":"archive|chart|page|gone|error","kind"?,"format"?,"size","charts","title"?,"error"?,"at"}
```

예: `bmseeker event works 152 --overrides overrides.json > works.jsonl`,
`bmseeker event check works.jsonl --known links.jsonl --minutes 290 > links.new.jsonl`.

다른 링크를 찾는 곳(소스)은 설정의 `sources` 순서로 묻습니다: 난이도표(`table`, `--also`로 더), Stella 업로더
목록(`stella`), TSV 목록(`tsv`, `--list`; `md5`, `url_diff`, `url`), [BMS SEARCH](https://bmssearch.net)(`bmssearch`,
채보 → 작품 → 작가가 올린 다운로드 링크, 작품에 묶이지 않은 차분은 제목·아티스트로 검색). `--own-links`: 자기 링크만.

설정(`--config`, `$BMSEEKER_CONFIG`, `<설정 폴더>/bmseeker/config.json`)은 JSON으로 기본값 위에 덮습니다:
`user_agent`, `mirror_catalog`(BMS 이벤트 미러 목록, 비우면 안 씀), `manbow`, `bmssearch_api`, `stella_list`,
`mega_api`, `sources`, `tables`, `downloads`(라이브러리 안 대기 폴더), `min_coverage`.

`cargo run -p bmseeker-cli -- table missing https://stellabms.xyz/sl/table.html -L ~/bms -l 0`

## 창

egui로 만든 창(`bmseeker-gui`)은 내렸습니다. CLI를 품는 Go(MyGo) GUI로 다시 만듭니다 ([DESIGN.md](DESIGN.md)).

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

네트워크를 읽는 것은 모두 `net::Task`(요청을 내놓고 응답을 받는 IO 없는 작업)입니다:
`hosts::Resolver`(링크를 `Host` 전략들로), `table::TableLoader`, `bmssearch::Lookup`. 네이티브는
`net::run`으로 블로킹, 브라우저(vibeBMS 웹 빌드)는 비동기로 같은 작업을 돌립니다. 읽는 곳(API, 미러,
목록)과 사용자 에이전트는 모두 `config::Config`에 있습니다. 앞으로의 구조는 [DESIGN.md](DESIGN.md).

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
