//! A manifest built from outside the crate, as the runtime binary builds it.

use speedyspec_runtime_contract::RuntimeManifest;

#[test]
fn a_manifest_is_built_through_its_constructor() {
    let manifest =
        RuntimeManifest::new("1.2.3".parse().unwrap(), vec![1], "8.5.12").with_sodium("1.0.22");
    assert_eq!(manifest.version.to_string(), "1.2.3");
    assert_eq!(manifest.contracts, vec![1]);
    assert_eq!(manifest.php, "8.5.12");
    assert_eq!(manifest.sodium.as_deref(), Some("1.0.22"));
    assert_eq!(
        serde_json::to_string(&manifest).unwrap(),
        r#"{"version":"1.2.3","contracts":[1],"php":"8.5.12","sodium":"1.0.22"}"#
    );
}

#[test]
fn a_new_manifest_has_no_sodium_until_given_one() {
    let manifest = RuntimeManifest::new("1.2.3".parse().unwrap(), vec![1], "8.5.12");
    assert_eq!(manifest.sodium, None);
    assert_eq!(
        serde_json::to_string(&manifest).unwrap(),
        r#"{"version":"1.2.3","contracts":[1],"php":"8.5.12"}"#
    );
}
