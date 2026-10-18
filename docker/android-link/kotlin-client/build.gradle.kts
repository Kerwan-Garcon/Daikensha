plugins { kotlin("jvm") version "2.0.21" }

val app = rootDir.resolve("../../../android/app/src")

sourceSets {
    main {
        kotlin.srcDir(app.resolve("main/kotlin"))
        kotlin.include(
            "com/coucou/android/link/**",
            "com/coucou/android/core/ApprovalSheetPlan.kt",
            "com/coucou/android/core/ToolLabels.kt",
            "com/coucou/android/core/Anim.kt",
            "com/coucou/android/core/SafeText.kt",
            "android/annotation/**",
            "com/coucou/android/mochi/MochiTypes.kt",
            "com/coucou/android/mochi/outfit/**",
        )
        kotlin.exclude("com/coucou/android/mochi/outfit/ComposeGfx.kt")
        // The one Android annotation the link package uses (android.annotation.SuppressLint).
        kotlin.srcDir("stubs")
    }
    test {
        kotlin.srcDir(app.resolve("test/kotlin"))
        kotlin.include(
            "com/coucou/android/ReferenceFiles.kt",
            "com/coucou/android/link/RustDesktopInteropTest.kt",
        )
    }
}

dependencies {
    implementation("org.json:json:20240303")
    testImplementation("junit:junit:4.13.2")
}

kotlin { jvmToolchain(21) }

tasks.test {
    // Run from the repository so ReferenceFiles finds it, and let the interop variable through.
    workingDir = rootDir.resolve("../../..")
    environment("COUCOU_RUST_INTEROP", System.getenv("COUCOU_RUST_INTEROP") ?: "")
    testLogging { events("passed", "failed", "skipped"); exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL }
}
