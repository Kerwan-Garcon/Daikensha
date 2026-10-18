// The Android app's link client (android/app/src/main/kotlin/com/coucou/android/link) and its interop
// test, compiled as plain Kotlin/JVM straight from android/, which is not modified. The link package uses
// no Android API, so this needs neither the Android SDK nor the Android Gradle plugin; it is the fallback
// of scripts/docker-android-link.sh when the full Android build cannot run (no access to Google's Maven).
pluginManagement { repositories { gradlePluginPortal(); mavenCentral() } }
dependencyResolutionManagement { repositories { mavenCentral() } }
rootProject.name = "coucou-kotlin-client"
