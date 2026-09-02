#[test]
fn core_crate_has_no_infrastructure_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    for forbidden in ["sea-orm", "axum", "tokio", "reqwest"] {
        assert!(
            !manifest.contains(forbidden),
            "myriad-agent-rules must not depend on {forbidden}"
        );
    }
}
