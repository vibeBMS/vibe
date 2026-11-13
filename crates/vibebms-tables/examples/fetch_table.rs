//! `cargo run -p vibebms-tables --example fetch_table -- <url>`
struct Blocking;
impl vibebms_tables::Fetcher for Blocking {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let r = ehttp::fetch_blocking(&ehttp::Request::get(url))?;
        if r.ok {
            Ok(r.bytes)
        } else {
            Err(format!("HTTP {}", r.status))
        }
    }
}

fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://stellabms.xyz/sl/table.html".into());
    match vibebms_tables::fetch_table(&Blocking, &url) {
        Ok(t) => {
            println!(
                "{} [{}] entries={} courses={}",
                t.header.name,
                t.header.symbol,
                t.entries.len(),
                t.header.courses.len()
            );
            println!("levels: {:?}", t.levels());
        }
        Err(e) => eprintln!("error: {e}"),
    }
}
