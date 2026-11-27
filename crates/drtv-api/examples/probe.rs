//! A live read of DR's front page, for checking the model against the service by hand.
//!
//! `cargo run -p drtv-api --example probe` — needs network access and a Danish egress, and prints
//! what the desktop client would draw rather than what the JSON says.

use drtv_api::{api, card};

fn main() {
    let universe = drtv_api::auth::universe();
    let Some(page) = api::home(&universe) else {
        eprintln!("the front page did not answer");
        std::process::exit(1);
    };
    let mut shelves = card::shelves(&page, &std::collections::BTreeMap::new());
    println!("{} shelves", shelves.len());

    let pending = card::pending_list_ids(&shelves);
    if !pending.is_empty() {
        let lists = api::lists(&pending, &universe);
        card::fill_lists(&mut shelves, &lists, &std::collections::BTreeMap::new());
    }

    for shelf in &shelves {
        println!("\n— {} ({})", shelf.title, shelf.items.len());
        for c in shelf.items.iter().take(6) {
            println!(
                "   {:<40} {:<10} {}",
                c.title,
                c.kicker(),
                if c.poster.is_empty() { "(no art)" } else { "art" }
            );
        }
    }
}
