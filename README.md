# vibeBMS

Lunatic Rave 2 / beatoraja를 레퍼런스로 한 **크로스플랫폼 네이티브 BMS 플레이어**입니다.
Rust + wgpu(Vulkan/Metal/DX12/GL/WebGPU) + cpal로 작성되어 x86/ARM Windows·macOS·Linux와 **웹 브라우저(wasm)**에서 동작하고,
iPad·Android·Surface 같은 터치 기기를 위한 화면 컨트롤러 모드를 내장하고 있습니다.

- 실행하면 별도 설정창 없이 바로 선곡 화면으로 들어가며, **모든 설정은 게임 안에서** 변경합니다.
- 설정·DB·난이도표 캐시·테마는 모두 **실행파일 옆 폴더에 포터블하게** 저장됩니다.
- 실행파일이 있는 폴더 안의 BMS는 **자동으로 스캔**되고, 추가 폴더도 등록할 수 있습니다.

> 상태: 초기 개발 버전(0.1). 코어 엔진(파서·판정·게이지·LN/CN/HCN)은 단위 테스트로 검증되어 있고,
> 데스크톱(Linux)에서 실제 구동·스크린샷으로 확인했습니다. 모바일 빌드는 스캐폴딩만 되어 있으며 실기기 검증 전입니다.
> 아래 [구현 현황](#구현-현황)을 참고하세요.

## 빠른 시작

```sh
cargo run --release                     # 선곡 화면
cargo run --release -- path/to/song.bme --autoplay   # 차트 바로 플레이
python3 tools/gen_demo.py bms/demo      # 테스트용 자작 데모 차트 생성
```

릴리스 빌드된 실행파일을 원하는 폴더에 두고, 그 폴더 안(하위 폴더 포함)에 BMS 폴더를 넣으면 됩니다.
다른 위치의 BMS 폴더는 창에 **드래그 앤 드롭**하거나 `SETTINGS › LIBRARY`에서 추가합니다.

```text
<root>/vibebms(.exe)
<root>/config/config.toml        설정 (게임 내에서 편집, 직접 편집도 가능)
<root>/cache/songs.json          곡 스캔 캐시 (지워도 재생성)
<root>/scores/scores.json        플레이 기록
<root>/skins/<스킨>/*.lr2skin     LR2 플레이 스킨
<root>/cache/tables/*.json       난이도표 캐시
<root>/themes/<name>/theme.toml  커스텀 테마
<root>/fonts/*.ttf|otf|ttc       폰트 지정(선택)
<root>/<아무 폴더>/...            자동 스캔되는 BMS
```

실행 폴더에 쓰기 권한이 없으면(예: Program Files) 사용자 데이터 폴더로 대체하고,
환경변수 `VIBEBMS_ROOT`로 위치를 직접 지정할 수도 있습니다.

## 조작

| | 키보드 기본값 (beatoraja와 동일) |
|---|---|
| 1P 7키 | `Z S X D C F V`, 스크래치 `LShift`(↑) / `LCtrl`(↓) |
| 2P 7키 | `M K , L . ; /`, 스크래치 `RShift` / `RCtrl` |
| 9키(PMS) | `A S D F G H J K L` |
| 선곡 | ↑↓/스크래치 이동, Enter/1·3·5·7번 키 결정, Backspace/2번 키 뒤로, `/` 검색 |
| 선곡 단축키 | F2 RANDOM, F3 게이지, F4 LN 모드, F5 오토플레이, F6 1P/2P, F7/F8 하이스피드, F9 판정 프리셋, F12 재스캔 |
| 플레이 중 | ↑↓ 하이스피드(Duration), PgUp/PgDn SUDDEN+, ←→ 판정 오프셋, Esc 종료 |
| 공통 | F1/Tab 설정, F11 전체화면 |

- **컨트롤러(IIDX 전용 컨트롤러 포함)**: `SETTINGS › KEY CONFIG`에서 버튼을 눌러 학습시킵니다.
  아날로그 턴테이블 축은 회전 방향으로 ↑/↓ 스크래치 입력이 되며, 멈추면 떼어진 것으로 처리합니다.
- **터치 모드**: `SETTINGS › TOUCH`. Auto(모바일·터치 감지 시 활성) / On / Off.
  - **IIDX 배열**: 턴테이블(원을 문질러 회전, 방향 전환 시 BSS 종료 입력) + 흰 건반 4개/검은 건반 3개 2단 배치
  - **키보드 배열**: 한 줄 건반(스크래치는 위아래 스와이프)
  - 세로/가로 모두 싱글 플레이, 가로에서는 더블 플레이(턴테이블 2개) 지원, 1P/2P 방향 전환
  - 손가락을 옆 건반으로 미끄러뜨리면 건반이 전환되고, 멀티터치를 지원합니다.

## 기능

### 차트
- BMS/BME/BML/PMS, BMSON(사운드 슬라이스 포함)
- `#RANDOM/#IF/#ELSEIF/#ELSE/#ENDIF`(중첩), `#SWITCH/#CASE/#SKIP/#DEF`, `#BASE 62`
- BPM 변경(03/08), STOP, `#SCROLL`, 마디 길이, LNTYPE 1, `#LNOBJ`, `#LNMODE`, 지뢰(Dx/Ex), 투명 노트
- 5/7/10/14키, 9키(PMS) 자동 판별, Shift_JIS / EUC-KR / UTF-8 자동 인코딩 감지
- MD5/SHA-256 (난이도표·IR 매칭용)

### 플레이
- 판정 프리셋: **beatoraja**(모드별 판정폭 + #RANK/#DEFEXRANK 스케일링 규칙), **LR2**(#RANK별 판정폭), **IIDX**(라이트닝 모델 120Hz 프레임 단위: PG ±2F, GR ±4F, GD ±14F, BAD ±30F, 입력 타이밍 8.33ms 양자화)
- 판정 알고리즘: Combo / Duration / Lowest, 커스텀 판정폭(PG/GR/GD %)
- 게이지: ASSIST EASY, EASY, NORMAL, HARD, **EX-HARD**, HAZARD, 단위(CLASS/EX-CLASS/EXHARD-CLASS) — beatoraja/LR2/IIDX 규칙 세트
- **GAUGE AUTO SHIFT**: CONTINUE / HARD→GROOVE / BEST CLEAR / SELECT TO UNDER (모든 게이지 병렬 계산)
- **LN / CN / HCN(헬차지노트)**, 백스핀 스크래치, 지뢰, 공POOR, PMS 1회 POOR 규칙
- 배치 옵션: MIRROR, RANDOM, R-RANDOM, S-RANDOM, SPIRAL, H-RANDOM, ALL-SCR, RANDOM+, S-RANDOM+, DP FLIP, BATTLE
- 어시스트: AUTO-SCRATCH, LEGACY NOTE, 지뢰 제거, 오토플레이
- 하이스피드: 플로팅(메인 BPM 기준 Duration/그린넘버 고정) / CONSTANT, SUDDEN+ / HIDDEN+ / LIFT
- 클리어 램프(L-ASSIST~MAX), EX 스코어, DJ LEVEL, FAST/SLOW, 게이지 그래프, 타이밍 히스토그램
- 이미지 BGA(BMP/PNG/JPG, 레이어 블랙키, POOR BGA)
- 단위/코스 플레이(난이도표의 `course` — 게이지 이월, `grade_mirror`/`gauge_lr2`/`ln`/`cn`/`hcn` 제약)

### 난이도표 / IR
- bmstable 포맷(`<meta name="bmstable">` → header.json → data.json): **LR2식 발광BMS/통상 난이도표**,
  **Satellite / Stella** 기본 등록. 표의 레벨 폴더, 미설치 곡(다운로드 URL 표시), 코스 지원.
  ribbit.xyz가 내려가 있으면 미러로 자동 대체합니다.
- IR: `IrClient` 트레이트로 확장 가능.
  - **LR2IR** 랭킹 조회(읽기 전용)
  - **JSON IR 프로토콜**(beatoraja `IRScoreData`와 같은 필드) — 점수 전송/랭킹/로그인. [docs/IR_PROTOCOL.md](docs/IR_PROTOCOL.md)
  - beatoraja의 IR은 Java 플러그인(jar)이라 네이티브 앱에서 그대로 로드할 수 없습니다. 각 IR 서비스는 어댑터를 추가해야 합니다.

### 오디오 / 성능
- cpal 저지연 출력(버퍼 크기 설정, Windows ASIO / Linux JACK 기능 플래그), 샘플 단위로 정확한 BGM/오토 키음 스케줄링
- 같은 키음 재발음 시 이전 음 컷(BMS 동작), 512 보이스, 소프트 클리핑
- 키음 병렬 디코딩(WAV/OGG/MP3/FLAC, 깨진 RIFF 헤더 폴백, `a.wav`→`a.ogg` 대체 탐색)
- 렌더러는 단일 파이프라인 배치 렌더링, 저지연 프레젠테이션(Mailbox/Immediate 우선), 프레임 제한 옵션

### 기본 스킨
실행 파일에 내장된 기본 플레이 스킨(beatoraja JSON 형식)이 모든 모드에 쓰입니다: 가로 SP(7/5키, 1P·2P 각각 레이아웃),
9키(필드 중앙 배치, 2열 원형 버튼), 가로 DP(14/10키, 양쪽 필드 모두 마디선), 세로 SP(7/5/9키, BGA를 노트 필드 뒤에 깔고
하단에 점수/판정 정보). PGREAT은 LR2/beatoraja 관례대로 깜빡이는 GREAT이며 콤보 숫자도 같은 주기·색으로 함께 깜빡입니다.
게이지는 노트 필드 폭 전체를 쓰고 값(`55.2%`)은 오른쪽 위에, 판정 표는 판정별 개수·FAST·SLOW와 CB(콤보 브레이크,
beatoraja 숫자 ID 425로도 제공) 및 FAST/SLOW 합계를 보여줍니다.
모든 모드가 같은 배치 규칙을 따릅니다: 노트 필드 바로 옆에 데이터 열(SCORE+스코어 그래프 → JUDGE → BPM → TIME/FPS),
그 바깥에 미디어 열(곡 제목 → BGA, 공간이 남으면 BGA 아래 BPM). 패널 크기는 모든 모드에서 같고(그래프만 남는 높이를 사용),
판정선·키·게이지·DURATION 줄의 높이도 가로 모드 전체에서 같습니다. DP는 beatoraja 기본 스킨처럼 두 필드를 좁은 간격으로 가운데에 모으고(간격에 진행 바, 그 아래 게이지) 왼쪽 열에 제목·BGA·판정·BPM, 오른쪽 열에 SCORE+그래프·TIME을 두며(판정선도 더 낮음),
세로 모드는 하단 한 줄(SCORE · JUDGE · BPM)에 같은 순서로 배치됩니다. BPM 패널에는 현재 하이스피드(HS, beatoraja 방식
`240000 / (BPM × 레인 전체 통과 시간 ms)`, 플로팅은 메인 BPM·고정 모드는 현재 BPM 기준, 숫자 ID 310/311)도 표시됩니다. 오차 0 판정은 FAST/SLOW 어느 쪽에도 세지 않습니다. 화면이 세로면 세로 레이아웃,
`1P/2P` 설정에 따라 필드 위치가 자동으로 바뀝니다. 노트 필드+턴테이블/건반+게이지, BGA, 점수/그래프/판정 카운트/BPM 등
장르의 일반적인 배치 관례만 따랐고, 아트는 모두 `tools/gen_default_skins.py`가 도형·그라디언트·Inter 폰트(OFL)로 절차적으로
그린 오리지널입니다(특정 게임의 그래픽·로고는 사용하지 않음). 수정하려면 스크립트를 고쳐 다시 실행하면
`crates/vibebms/assets/builtin/skins/default/`가 재생성됩니다. 선택한 스킨을 읽지 못하면 내장 기본 스킨을 씁니다.

### 선곡 / 결과 스킨
선곡과 결과 화면도 스킨으로 그립니다(`SETTINGS › DISPLAY › Select skin / Result skin`, DEFAULT → 설치된 스킨).
내장 기본 스킨은 가로·세로 버전이 있고, LR2(`#SRC_BAR_BODY`/`#DST_BAR_BODY_ON/OFF`/`#BAR_CENTER`/`#BAR_AVAILABLE`/
`#DST_BAR_TITLE`/`#SRC_BAR_LEVEL`/`#SRC_BAR_LAMP`, `#SRC_GAUGECHART`)와 beatoraja(`songlist`, `imageset`, `gaugegraph`,
`judgegraph`, `bpmgraph`, `timingdistributiongraph`, 클릭 가능한 `act` 이미지, `ref` 이미지)를 읽습니다. 숫자·옵션·타이머 ID는
beatoraja/LR2와 같고, vibeBMS 전용 선곡 옵션은 텍스트 `9000+i`(값) / `9100+i`(이름)와 클릭 이벤트 `9000+i`로 노출됩니다.
LR2 `#SRC_BUTTON`(값별 프레임, 클릭 시 버튼 ID 실행, 패널 소속)과 패널(버튼 1~9로 열고 닫기, 타이머 21~26/31~36,
옵션 21~23, Back으로 닫기, 패널이 있는 스킨은 옵션 키가 패널 1을 엽니다)을 지원합니다. 버튼 ID: 15 플레이, 16 오토,
13/14 설정, 40 게이지, 42/43 랜덤, 54 DP 옵션, 55 HS 고정, 72 BGA, 308 LN 모드. 결과 스킨은 9500(재시도)/9501(계속),
선곡 스킨은 9900(상위 폴더) 클릭 이벤트를 씁니다.
beatoraja 그래프의 색·선 속성(`gaugegraph`의 `color` 24색 목록 또는 `*BGColor`/`*LineColor`, `bpmgraph`의
BPM별 색·`lineWidth`, `timingdistributiongraph`의 `width`·판정 구간 색·`drawAverage`/`drawDev`, `judgegraph`의
`noGap`/`orderReverse`, `delay`)을 따릅니다. 그래프 객체에 `"style": "vibebms"`를 주면 vibeBMS 자체 그래프 모양
(FAST/SLOW 색, AVG/SD 표시 등)으로 그립니다 — 내장 기본 스킨이 이 방식입니다.

### 스킨 (LR2 / beatoraja)
`skins/` 아래에 LR2 스킨 폴더를 넣고(`LR2files\Theme\…` 경로는 `skins/` 기준으로 해석) `SETTINGS › DISPLAY › Play skin (7KEYS 등)`에서 선택합니다
(DEFAULT → 설치된 스킨 순으로 순환).
지원: `#IMAGE`, `#SRC/#DST_IMAGE`(키프레임·가속·루프·타이머·조건·회전·가산 블렌딩), `_NUMBER`, `_TEXT`(시스템 폰트), `_BARGRAPH`,
`_SLIDER`, `_GROOVEGAUGE`, `_BGA`, `_NOTE`/`_LN_*`/`_MINE`, `_LINE`, `_JUDGELINE`, `_NOWJUDGE`/`_NOWCOMBO`, `#IF/#ELSE`, `#CUSTOMOPTION`(기본값),
`#CUSTOMFILE`, `#INCLUDE`. 미지원: `#LR2FONT` 비트맵 폰트, 버튼/마우스 오버, 선곡/결과 스킨.

beatoraja 플레이 스킨도 같은 메뉴에서 선택합니다: **JSON**(`.json`, 주석·trailing comma 허용, `if`/`include` 전처리, 키프레임 값 상속, y-up 좌표 변환)과
**Lua**(`.luaskin`, 내장 Lua 5.4로 실행 — 데스크톱/모바일 전용, 웹 미지원).
Lua 함수(또는 JSON 스킨의 Lua 스크립트 문자열)로 지정된 값 — `draw`/`op` 조건, `timer`, 숫자·텍스트·이미지셋 `value`,
슬라이더/그래프 비율, 클릭 `act`, `customTimers`/`customEvents` — 은 매 프레임 평가됩니다. beatoraja와 같은
`main_state`(option/number/float_number/text/timer/time/set_timer/event_exec/offset 등), `timer_util`, `event_util` 모듈과
`skin_config`(option, enabled_options, offset, file_path, get_path)를 제공하며, JSON 스킨에서는 이 함수들이 전역입니다.
스크립트는 샌드박스에서 실행됩니다(`os`/`io`/`debug`/네이티브 모듈 없음, `require`·`dofile`은 스킨 폴더 안의 Lua 파일만).
스킨 커스텀 옵션은 `SETTINGS › DISPLAY`에서
각 스킨 바로 아래 항목으로 고르며 `config.skin_options`에 저장됩니다.

### 웹 버전
`tools/build_web.sh` → `web/`을 HTTP로 서빙. 좌하단 **📂 Open BMS folder**로 폴더를 고르면 브라우저 안에서 스캔·플레이합니다(파일은 업로드되지 않음).
설정·기록은 localStorage에 저장. 웹에는 시스템 폰트가 없으므로 일본어/한국어 제목을 보려면 `web/fonts/`에 CJK 폰트를 넣고 `fonts.txt`에 나열하세요.
난이도표/IR은 해당 서버가 CORS를 허용해야 동작합니다.

### 테마
`default` 테마는 내장 값(기본 스킨과 같은 팔레트)을 쓰며, 그 값이 `themes/default/theme.toml.example`로 기록됩니다.
새 폴더(`themes/<이름>/theme.toml`)에 복사해 메뉴 색상·터치 버튼 폭·배경 이미지를 바꾸고(게임 화면은 스킨이 그립니다)
`SETTINGS › DISPLAY › Theme`에서 선택하세요. 테마 폴더의 `fonts/`에 폰트를 넣으면 우선 사용됩니다.

## 구현 현황

| 영역 | 상태 |
|---|---|
| 코어 엔진(파서/타이밍/판정/게이지/점수/랜덤) | ✅ 구현 + 단위 테스트 |
| 데스크톱 앱(선곡/플레이/결과/설정/터치) | ✅ Linux에서 구동 확인, Windows 크로스 컴파일 확인, macOS는 CI 빌드 |
| 웹(wasm, WebGPU/WebGL2) | ✅ 헤드리스 크로미움에서 폴더 선택→스캔→플레이 확인 |
| ARM(Windows on ARM, Apple Silicon, Linux aarch64) | ✅ CI 매트릭스 |
| Android | 🧪 `crates/vibebms-android` (cargo-apk) — 실기기 미검증 |
| iOS/iPadOS | 🧪 빌드 가이드 + Info.plist — 실기기 미검증 |
| LR2 플레이 스킨 | 🧪 주요 명령 구현, 합성 스킨으로 검증 (실제 공개 스킨 호환성은 추가 검증 필요) |
| beatoraja JSON/Lua 스킨 | 🧪 구조 + Lua 런타임 함수(조건·타이머·값·이벤트) 평가, 합성 스킨으로 검증 (실제 공개 Lua 스킨은 미검증) |
| 내장 기본 스킨(가로 SP 1P/2P, DP, 세로 SP) | ✅ 7/5/9/14/10키 레이아웃, 로드 테스트 + 스크린샷 확인 |
| 선곡/결과 스킨 (LR2·beatoraja, 내장 기본 가로/세로) | 🧪 곡 목록·정보·그래프·버튼·패널 지원, beatoraja 기본 스킨(플레이/선곡/결과)으로 확인 |
| 스킨 옵션 선택 UI | ✅ `SETTINGS › DISPLAY`에서 선택한 스킨 아래에 표시(LR2 `#CUSTOMOPTION`, beatoraja `property`) |
| 동영상 BGA(mpg/wmv/mp4) | ⏳ 미구현 |
| 리플레이 저장/재생, 고스트 배틀, 타깃 스코어 | ⏳ 미구현 (엔진에 ghost 데이터는 기록됨) |
| 특정 IR 서비스 전용 어댑터(MochaIR 등) | ⏳ 프로토콜 확인 후 추가 필요 |

판정·게이지 상수와 출처, 근사치인 부분은 [docs/RULES.md](docs/RULES.md)에 정리했습니다.
빌드 방법(크로스 컴파일·모바일)은 [docs/BUILDING.md](docs/BUILDING.md)를 보세요.

## 구조

```text
crates/vibebms-core     파서, 타이밍, 판정, 게이지, 점수, 랜덤, 판정 엔진 (플랫폼 의존성 없음)
crates/vibebms-tables   난이도표 로더
crates/vibebms-ir       IR 클라이언트
crates/vibebms-audio    디코더 + 믹서
crates/vibebms          앱 (winit + wgpu)
crates/vibebms-android  Android 패키징
```

## 라이선스
`MIT OR Apache-2.0` (LICENSE-MIT, LICENSE-APACHE). 내장 폰트 Inter는 SIL OFL 1.1 (`crates/vibebms/assets/fonts/OFL.txt`).

## 크레딧
판정·게이지 규칙은 beatoraja(exch-bms2, GPL-3.0)의 공개 소스와 LR2 커뮤니티에 공개된 판정폭을 참고해
독자적으로 구현했습니다. 차트·음원은 포함하지 않습니다(`tools/gen_demo.py`는 자체 생성 데모).
