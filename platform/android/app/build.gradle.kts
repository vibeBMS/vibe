// The Rust library (libvibebms_android.so) is built by cargo-ndk into
// app/jniLibs/<abi>/ before this runs (see docs/BUILDING.md).
plugins {
    id("com.android.application")
}

android {
    namespace = "io.github.vibebms"
    compileSdk = 34

    defaultConfig {
        // each channel is its own app (tools/ci_version.sh): CI's nightly
        // is io.github.vibebms.nightly, "vibeBMS Nightly"
        applicationId = System.getenv("VIBEBMS_APP_ID") ?: "io.github.vibebms"
        manifestPlaceholders["appName"] = System.getenv("VIBEBMS_APP_NAME") ?: "vibeBMS"
        minSdk = 26
        targetSdk = 34
        // CI builds: the run number and <version>-<channel>.<run>+<commit>
        versionCode = (System.getenv("VIBEBMS_VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("VIBEBMS_VERSION_NAME") ?: "0.1.0"
    }

    // the release APK is built unsigned (app-release-unsigned.apk), and
    // tools/build_release.sh signs it with apksigner: so a rebuild, which has
    // no key, makes the very APK the release's signature was made over, and
    // the release's signature copied onto it gives the release byte for byte
    // (reproducible builds, docs/BUILDING.md)
    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = null
        }
    }

    // no dependency list in the APK signing block (encrypted for Google
    // Play, which this APK does not go to; F-Droid flags it)
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    sourceSets["main"].jniLibs.srcDirs("jniLibs")

    // the library as cargo built it (stripped already): AGP strips only with
    // an NDK of its own version, so the APK would depend on the NDKs the
    // machine has installed (reproducible builds, docs/BUILDING.md)
    packaging {
        jniLibs {
            keepDebugSymbols += "**/*.so"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
