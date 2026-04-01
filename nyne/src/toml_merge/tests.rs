use super::*;

#[test]
fn scalar_overwrite() {
    let mut base: toml::Value = toml::toml! { key = "base" }.into();
    let overlay: toml::Value = toml::toml! { key = "overlay" }.into();
    deep_merge(&mut base, &overlay);
    assert_eq!(base["key"].as_str(), Some("overlay"));
}

#[test]
fn nested_table_merge() {
    let mut base: toml::Value = toml::toml! {
        [section]
        a = 1
        b = 2
    }
    .into();
    let overlay: toml::Value = toml::toml! {
        [section]
        b = 99
        c = 3
    }
    .into();
    deep_merge(&mut base, &overlay);
    let section = base["section"].as_table().unwrap();
    assert_eq!(section["a"].as_integer(), Some(1));
    assert_eq!(section["b"].as_integer(), Some(99));
    assert_eq!(section["c"].as_integer(), Some(3));
}

#[test]
fn array_replaced_not_extended() {
    let mut base: toml::Value = toml::toml! { tags = ["a", "b"] }.into();
    let overlay: toml::Value = toml::toml! { tags = ["c"] }.into();
    deep_merge(&mut base, &overlay);
    let tags: Vec<&str> = base["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(tags, vec!["c"]);
}

#[test]
fn new_keys_added() {
    let mut base: toml::Value = toml::toml! { existing = true }.into();
    let overlay: toml::Value = toml::toml! { new_key = "hello" }.into();
    deep_merge(&mut base, &overlay);
    assert_eq!(base["existing"].as_bool(), Some(true));
    assert_eq!(base["new_key"].as_str(), Some("hello"));
}

#[test]
fn deeply_nested_merge() {
    let mut base: toml::Value = toml::toml! {
        [a.b.c]
        value = 1
    }
    .into();
    let overlay: toml::Value = toml::toml! {
        [a.b.c]
        value = 2
        extra = true
    }
    .into();
    deep_merge(&mut base, &overlay);
    assert_eq!(base["a"]["b"]["c"]["value"].as_integer(), Some(2));
    assert_eq!(base["a"]["b"]["c"]["extra"].as_bool(), Some(true));
}
