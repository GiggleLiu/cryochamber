const GRADLE: &str = include_str!("../gen/android/app/build.gradle.kts");
const MANIFEST: &str = include_str!("../gen/android/app/src/main/AndroidManifest.xml");
const RELEASE_WORKFLOW: &str = include_str!("../../../.github/workflows/release.yml");
const BRAND_ICON: &[u8] = include_bytes!("../icons/android/mipmap-xxxhdpi/ic_launcher.png");
const BRAND_FOREGROUND: &[u8] =
    include_bytes!("../icons/android/mipmap-xxxhdpi/ic_launcher_foreground.png");
const BRAND_ROUND: &[u8] = include_bytes!("../icons/android/mipmap-xxxhdpi/ic_launcher_round.png");
const ANDROID_ICON: &[u8] =
    include_bytes!("../gen/android/app/src/main/res/mipmap-xxxhdpi/ic_launcher.png");
const ANDROID_FOREGROUND: &[u8] =
    include_bytes!("../gen/android/app/src/main/res/mipmap-xxxhdpi/ic_launcher_foreground.png");
const ANDROID_ROUND: &[u8] =
    include_bytes!("../gen/android/app/src/main/res/mipmap-xxxhdpi/ic_launcher_round.png");

#[test]
fn android_release_stays_signed_and_allows_confirmed_http_hubs() {
    assert!(GRADLE.contains("keystore.properties"));
    assert!(GRADLE.contains("signingConfig = signingConfigs.getByName(\"release\")"));
    assert!(GRADLE.contains("manifestPlaceholders[\"usesCleartextTraffic\"] = \"true\""));
    assert!(RELEASE_WORKFLOW.contains("tauri android build --apk --target aarch64"));
    assert!(RELEASE_WORKFLOW.contains("apksigner\" verify --print-certs"));
}

#[test]
fn android_launcher_uses_the_cryochamber_mark() {
    assert_eq!(ANDROID_ICON, BRAND_ICON);
    assert_eq!(ANDROID_FOREGROUND, BRAND_FOREGROUND);
    assert_eq!(ANDROID_ROUND, BRAND_ROUND);
}

#[test]
fn android_opens_cryochamber_links_and_shared_text() {
    assert!(MANIFEST.contains("android.intent.action.VIEW"));
    assert!(MANIFEST.contains("android:scheme=\"cryochamber\" android:host=\"add\""));
    assert!(MANIFEST.contains("android.intent.action.SEND"));
    assert!(MANIFEST.contains("android:mimeType=\"text/plain\""));
}
