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
bmseeker place <채보> <라이브러리> [--to 폴더] [--min 퍼센트]
                                                     차분이 들어갈 곡 폴더 후보 (또는 옮기기)
bmseeker pending <라이브러리>                         곡을 기다리는 차분
bmseeker export lr2 <난이도표 URL> <폴더>             LR2 커스텀 폴더 (그 폴더를 LR2에 추가)
bmseeker export bmt <난이도표 URL> <beatoraja table 폴더>
                                                     beatoraja 난이도표 캐시
```

`missing`과 `export`는 비었거나 죽은 링크를 MD5로 채웁니다(`--own-links`: 난이도표 그대로).

`cargo run -p bmseeker -- missing https://stellabms.xyz/sl/table.html ~/bms -l 0`

## 모듈

| 모듈 | 하는 일 |
| --- | --- |
| `table` | bmstable 형식 난이도표 (HTML의 `<meta name="bmstable">` → header → data) |
| `hosts` | 링크 → 아카이브: 직링크, Google Drive 파일·폴더, Dropbox, MediaFire, MEGA 파일·폴더(합본 안에서 제목·아티스트로 찾기), manbow 이벤트 작품 페이지 |
| `unpack` / `import` | .zip / .7z / .rar 풀기 (Shift_JIS·EUC-KR 이름, 경로 탈출·압축 폭탄 방지) |
| `placement` | 차분 채보가 어느 곡 폴더에 속하는지: 함께 온 파일까지 쳐서 쓰는 음원의 99% 이상 + 같은 곡(제목·아티스트), 아니면 후보 3개; 같은 이름 파일은 스마트 덮어쓰기 |
| `install` | 난이도표의 곡 하나: 본체 링크와 차분 링크를 받아 라이브러리에 |
| `links` | 비었거나 죽은 링크를 차트 MD5로 채우기 (BeMusicSeeker 비공식 포크의 MD5-URL 표, Stella 업로더 목록) |
| `export` | 난이도표를 LR2 커스텀 폴더(`.lr2folder`)나 beatoraja 캐시(`.bmt`)로 |
| `fs` | 파일 접근 (`Disk`, 또는 플레이어가 라이브러리를 두는 곳) |

네트워크 부분은 IO 없는 단계 기계(`hosts::Resolver`, `table::TableLoader`)라, 블로킹으로도
(네이티브의 `hosts::fetch`) 브라우저 비동기로도(vibeBMS 웹 빌드) 같은 로직을 돌립니다.

## 출처

설치 폴더 추정·스마트 덮어쓰기·링크 보완·내보내기의 규칙은
[BeMusicSeeker](https://github.com/Neeted/bemusicseeker-unofficial-fork)(rib_2_bit 원작, Neeted 비공식 포크, MIT)의
동작을 따라 Rust로 새로 구현했습니다. 링크 보완은 그 포크가 공개한 MD5-URL 표를 실행 중에 받아 씁니다.

## 라이선스

`MIT OR Apache-2.0`. `.rar`는 [unrar-rs](https://github.com/scryer-media/rarpar/tree/main/crates/unrar-rs)
(Apache-2.0, UnRAR 라이선스 제한 포함)로 풉니다.

> UnRAR source code may be used in any software to handle RAR archives without limitations free of charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR compression algorithm, which is proprietary. Distribution of modified UnRAR source code in separate form or as a part of other software is permitted, provided that full text of this paragraph, starting from "UnRAR source code" words, is included in license, or in documentation if license is not available, and in source code comments of resulting package.

웹(wasm)에서 쓰려면 워크스페이스의 `[patch.crates-io] unrar-rs` 패치가 필요합니다
(`third_party/unrar-rs/VIBEBMS.md`).
