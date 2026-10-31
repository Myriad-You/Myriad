fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&myriad_tapp_contract::export_tapp_contract())
            .expect("contract must serialize")
    );
}
