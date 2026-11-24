use protocol::{ModelProfile, ToolCodec};
use serde::Deserialize;

#[derive(Deserialize)]
struct ModelTables {
    model: Vec<ModelProfile>,
}

fn example_profiles() -> Vec<ModelProfile> {
    let config = include_str!("../../../config/local-ide.example.toml");
    toml::from_str::<ModelTables>(config)
        .expect("example configuration decodes")
        .model
}

#[test]
fn example_configuration_lists_the_initial_profiles() {
    let ids: Vec<String> = example_profiles()
        .into_iter()
        .map(|profile| profile.id)
        .collect();

    assert_eq!(ids, ["coder", "fast"]);
}

#[test]
fn example_profile_decodes_each_field() {
    let coder = example_profiles()
        .into_iter()
        .find(|profile| profile.id == "coder")
        .expect("coder profile exists");

    assert_eq!(
        coder,
        ModelProfile {
            id: "coder".to_owned(),
            label: "Coder".to_owned(),
            role: "General coding".to_owned(),
            context_budget: 24000,
            reply_reserve: 8192,
            tool_codec: ToolCodec::Native,
            temperature: None,
            top_p: None,
            delegate_target: true,
            chars_per_token: 3.5,
        }
    );
}

#[test]
fn each_example_profile_reserves_less_than_its_budget() {
    for profile in example_profiles() {
        assert!(
            profile.reply_reserve < profile.context_budget,
            "{} reserves its whole budget",
            profile.id
        );
    }
}

#[test]
fn example_profiles_hold_the_codec_and_delegate_flag() {
    let profiles: Vec<(String, ToolCodec, bool)> = example_profiles()
        .into_iter()
        .map(|profile| (profile.id, profile.tool_codec, profile.delegate_target))
        .collect();

    let expected = [
        ("coder", ToolCodec::Native, true),
        ("fast", ToolCodec::Text, true),
    ]
    .map(|(id, codec, delegate)| (id.to_owned(), codec, delegate));
    assert_eq!(profiles, expected);
}
