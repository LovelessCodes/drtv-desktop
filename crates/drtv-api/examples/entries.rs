fn main() {
    let page = drtv_api::api::home(&drtv_api::auth::Universe::general()).unwrap();
    let item = page
        .entries
        .iter()
        .filter_map(|e| e.list.as_ref())
        .flat_map(|l| l.items.iter())
        .find(|i| i.id == "623436")
        .unwrap();
    let full = drtv_api::api::expanded(&item.id).unwrap();
    for ep in full.episodes.items.iter().take(3) {
        println!("title={:?} episode_name={:?}", ep.title, ep.episode_name);
    }
}
