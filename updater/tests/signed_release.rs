//! Opt-in verification against a real published keyless signature. No Docker mutations.
//! Run with the same cosign v2.4.1 used by updater/Dockerfile on PATH:
//! cargo test --test signed_release -- --ignored --nocapture
use myriad_updater::release::{CosignPolicy, GithubClient, VerifyOutcome, cosign};

#[tokio::test]
#[ignore = "requires cosign and network access to GitHub/Fulcio/Rekor"]
async fn published_release_verifies_and_tampering_or_foreign_identity_fails() {
    let dir = tempfile::tempdir().unwrap();
    let tag = "v0.5.7";
    let github = GithubClient::new(
        "Myriad-You/Myriad",
        None,
        dir.path().to_owned(),
        CosignPolicy::Strict,
    )
    .unwrap();
    let manifest = github.fetch_manifest(tag).await.unwrap();
    assert_eq!(manifest.version.as_str(), tag);
    let blob = dir.path().join(format!("release-{tag}.json"));
    let signature = dir.path().join(format!("release-{tag}.sig"));
    let certificate = dir.path().join(format!("release-{tag}.pem"));
    let original = std::fs::read(&blob).unwrap();
    // Even a valid JSON edit must fail cryptographic verification.
    let mut tampered = original.clone();
    tampered.push(b'\n');
    std::fs::write(&blob, tampered).unwrap();
    let outcome = cosign::verify(&blob, &signature, &certificate, "Myriad-You/Myriad").await;
    assert!(!matches!(outcome, VerifyOutcome::Verified));
    assert!(cosign::enforce(&outcome, CosignPolicy::Strict).is_err());
    std::fs::write(&blob, &original).unwrap();
    let outcome = cosign::verify(&blob, &signature, &certificate, "Myriad-You/Other").await;
    assert!(!matches!(outcome, VerifyOutcome::Verified));
    assert!(cosign::enforce(&outcome, CosignPolicy::Strict).is_err());
    std::fs::remove_file(&signature).unwrap();
    let outcome = cosign::verify(&blob, &signature, &certificate, "Myriad-You/Myriad").await;
    assert!(cosign::enforce(&outcome, CosignPolicy::Strict).is_err());
    println!(
        "PASS: published keyless signature, tampered JSON, foreign identity and missing signature"
    );
}
