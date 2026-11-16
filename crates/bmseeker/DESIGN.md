# bmseeker: design

Where this crate is going before it moves to its own repository
(`vibeBMS/bmseeker`): a library with no hard-coded places, a CLI whose
commands read and write streams, and a GUI (Go, MyGo) on top of the CLI.

## Layers

```
bmseeker (lib)      sans-IO core: tasks that ask for requests and are fed the answers
  net               Request / Next / Task, the Http trait, run() (native driver)
  config            Config: every endpoint, the user agent, folders, rules
  hosts             Host strategies: a link -> a file URL, another link, or a page
  sources           Source strategies: a song (md5 / title / artist) -> links
  event             manbow events: works and checks as JSON Lines records
  table, unpack, import, placement, install, export, probe (as now)
bmseeker-cli        clap, noun-verb commands, --json (JSON Lines), `serve`
gui/ (Go, MyGo)     embeds the CLI; with a CLI command it is the CLI
```

### Tasks (`net`)

Everything that talks to the network is a `Task`: `start()` and
`feed(answer)` return `Next::Ask(Request)` or `Next::Done(output)`. A task
never does I/O, so the same code runs natively (`net::run` with an `Http`),
in the web build (vibe's fetch through its CORS proxy) and in tests (canned
answers). Downloading a file's bytes is not a task: the result names the
file (`Target { url, decrypt, name }`) and the caller streams it.

### Hosts (strategy)

```rust
trait Host {
    fn name(&self) -> &'static str;
    fn accepts(&self, url: &Link) -> bool;
    fn open(&self, url: &Link, cx: &Context) -> Box<dyn Task<Output = Resolved>>;
}
enum Resolved { File(Target), Follow(Vec<String>), Page, Gone, Failed(String) }
```

`Resolver` walks a registry (`Hosts::default()`: Google Drive file and
folder, Dropbox, MediaFire, MEGA file and folder, manbow work (the event
mirror first), BMS SEARCH venue, browser-only hosts, direct). `Follow`
starts again with the next link (at most a few hops). A host is one
struct; adding one is adding it to the registry.

### Sources (strategy)

```rust
trait Source {
    fn name(&self) -> &'static str;
    fn find(&self, q: &Query, cx: &Context) -> Box<dyn Task<Output = Vec<Candidate>>>;
}
struct Query { md5: Option<String>, title: String, artist: String }
struct Candidate { url: String, diff: bool, source: &'static str }
```

Difficulty tables, the Stella uploader's list, TSV lists, BMS SEARCH, the
event mirror. Which and in what order: `Config::sources`.

### Config

All endpoints have defaults in `Config::default()` and can be set by a
JSON file (`--config`, `BMSEEKER_CONFIG`, the config folder) or flags:
`bmssearch_api`, `stella_list`, `mirror_catalog`, `manbow`, `mega_api`,
`user_agent`, `downloads` (folder name), `sources`, `placement` rules.

## CLI

`bmseeker [--json] [--config FILE] <noun> <verb> ...`. With `--json` every
command writes JSON Lines (one record per line) and reads records from
stdin where it takes a list (`-`).

```
link resolve <LINK>...                     what each link leads to
link get <LINK>... [-o DIR] [--title T --artist A --md5 M] [--search] [--any]
song find [--md5 M] [--title T] [--artist A]
table show <URL>
table missing <URL> --library DIR [--level L] [--get]
table export lr2|bmt <URL> <OUT>
library import <FILE> --library DIR
library place <CHART> --library DIR [--to DIR] [--min PERCENT]
library pending --library DIR
library install <LINK>... --library DIR [--diff] [--title T --artist A --md5 M]
event works <VENUE> [--overrides FILE]     works of a manbow event (JSONL)
event check [FILE|-] [--known FILE]        each work's links checked (JSONL)
serve                                      requests and events over stdio (GUI)
```

### Records

```
work   {"num","title","artist","team","kind","size_kb","page",
        "links":[{"label","url"}],"main":[url],"extra":[[url]],"review"?}
check  {"url","state":"archive|chart|page|gone|error","kind"?,"format"?,
        "size","charts","title"?,"error"?,"at"}
link   {"link","state":"file|page|gone|error","url"?,"name"?,"error"?}
got    {"file","name","format","link","errors":[...]}
```

`--overrides` takes work records (JSON or JSONL) whose fields replace the
guess for that `num`. The BMS mirror keeps whatever files it likes and
pipes them through.

## GUI

Go with MyGo, native UI. It runs the CLI (`serve`) as a child process.
Released as two binaries: `bmseeker` (CLI) and the GUI, which embeds the
CLI: started with a CLI command it runs as the CLI (same arguments,
stdin/stdout, exit code); with no arguments or a path it opens the window.
A link that needs a browser opens in a MyGo web view, which presses the
page's Download button and hands the download back to the CLI.
