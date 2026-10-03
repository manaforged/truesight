mod support;

use support::{CONFIG, Fixture};

#[test]
fn items_prints_each_item_list_line_with_its_kind_and_path() {
    let fixture = Fixture::new("items");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "api/fixture.txt",
        "pub mod fixture\npub fn fixture::get(id: u8) -> u8\npub fixture::Token::value: u8\nimpl core::clone::Clone for fixture::Token\n",
    );
    let run = fixture.succeed(&["items"]);
    let listings: serde_json::Value = serde_json::from_str(&run.stdout).expect("items prints JSON");
    assert_eq!(listings[0]["file"], "api/fixture.txt");
    let pairs: Vec<(String, String)> = listings[0]["items"]
        .as_array()
        .expect("an item array")
        .iter()
        .map(|item| {
            (
                item["kind"].as_str().expect("a kind").to_owned(),
                item["path"].as_str().expect("a path").to_owned(),
            )
        })
        .collect();
    assert_eq!(
        pairs,
        [
            ("mod", "fixture"),
            ("fn", "fixture::get"),
            ("field", "fixture::Token::value"),
            ("impl", "fixture::Token"),
        ]
        .map(|(kind, path)| (kind.to_owned(), path.to_owned()))
    );
}
