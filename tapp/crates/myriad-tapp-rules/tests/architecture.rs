#[test]
fn core_crate_has_no_infrastructure_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    for forbidden in ["sea-orm", "axum", "tokio", "reqwest"] {
        assert!(
            !manifest.contains(forbidden),
            "myriad-tapp-rules must not depend on {forbidden}"
        );
    }
}

#[test]
fn depends_on_tapp_contract() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        manifest.contains("myriad-tapp-contract"),
        "myriad-tapp-rules must depend on tapp-contract"
    );
}
