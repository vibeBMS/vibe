//! `cargo run -p vibebms-tables --example fetch_table -- <url>`
fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://stellabms.xyz/sl/table.html".into());
    match vibebms_tables::fetch_table(&vibebms_tables::UreqFetcher::default(), &url) {
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
