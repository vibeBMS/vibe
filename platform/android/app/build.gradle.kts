// The Rust library (libvibebms_android.so) is built by cargo-ndk into
// app/jniLibs/<abi>/ before this runs (see docs/BUILDING.md).
plugins {
    id("com.android.application")
}

// release signing: VIBEBMS_KEYSTORE (path), VIBEBMS_KEYSTORE_PASSWORD and
// VIBEBMS_KEY_ALIAS; without them the release APK gets the debug key
val keystore: String? = System.getenv("VIBEBMS_KEYSTORE")

android {
    namespace = "io.github.vibebms"
    compileSdk = 34

    defaultConfig {
        applicationId = "io.github.vibebms"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    signingConfigs {
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("VIBEBMS_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("VIBEBMS_KEY_ALIAS") ?: "vibebms"
                keyPassword = System.getenv("VIBEBMS_KEYSTORE_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    sourceSets["main"].jniLibs.srcDirs("jniLibs")

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
