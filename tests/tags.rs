use pasteleaf::{
    model::{Content, Model},
    store::Store,
};

#[test]
fn tags_survive_reopen_and_search_within_category() {
    let root = std::env::temp_dir().join(format!("pasteleaf-tags-{}", std::process::id()));
    let store = Store::new(root.clone()).unwrap();
    let mut original = Model::default();
    let id = original
        .add_favorite_text("synthetic body only", "收藏")
        .unwrap();
    original.add_category("其他").unwrap();
    let mut json = serde_json::to_value(&original).unwrap();
    json["items"][0]["tags"] = serde_json::json!(["workbuddy2api", "密码"]);
    let mut model: Model = serde_json::from_value(json).unwrap();
    assert_eq!(model.visible("WORKBUDDY2API", Some("收藏")), vec![id]);
    assert!(model.visible("密码", Some("其他")).is_empty());
    model
        .capture(Content::Text("synthetic body only".into()))
        .unwrap();
    model.assign(id, None).unwrap();
    model.assign(id, Some("其他".into())).unwrap();
    store.save(&model).unwrap();
    let reopened = store.load().unwrap();
    assert_eq!(reopened.visible("密码", Some("其他")), vec![id]);
    assert_eq!(
        reopened.items[0].content,
        Content::Text("synthetic body only".into())
    );
    assert_eq!(
        serde_json::to_value(&reopened).unwrap()["items"][0]["tags"],
        serde_json::json!(["workbuddy2api", "密码"])
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn old_library_without_tags_still_loads() {
    let mut model = Model::default();
    model.add_favorite_text("old content", "收藏").unwrap();
    let mut json = serde_json::to_value(&model).unwrap();
    json["items"][0].as_object_mut().unwrap().remove("tags");
    let restored: Model = serde_json::from_value(json).unwrap();
    restored.validate().unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap()["items"][0]["tags"],
        serde_json::json!([])
    );
}
