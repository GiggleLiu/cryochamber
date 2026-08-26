const GRADLE: &str = include_str!("../gen/android/app/build.gradle.kts");
const RELEASE_WORKFLOW: &str = include_str!("../../../.github/workflows/release.yml");

#[test]
fn android_release_stays_signed_and_allows_confirmed_http_hubs() {
    assert!(GRADLE.contains("keystore.properties"));
    assert!(GRADLE.contains("signingConfig = signingConfigs.getByName(\"release\")"));
    assert!(GRADLE.contains("manifestPlaceholders[\"usesCleartextTraffic\"] = \"true\""));
    assert!(RELEASE_WORKFLOW.contains("tauri android build --apk --target aarch64"));
    assert!(RELEASE_WORKFLOW.contains("apksigner\" verify --print-certs"));
}
