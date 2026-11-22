// scan2fil's own Android code and manifest entries. dx bundles this folder as
// a Gradle module (see `sheet.rs`), and Android merges its manifest into the app's.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.mayorana.scan2fil.platform"
    compileSdk = 34
    defaultConfig {
        minSdk = 24
        // Applied to the app's release build too.
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    // FileProvider, for sharing the PDF. The app already ships it (through appcompat).
    implementation("androidx.core:core-ktx:1.13.1")
    // Custom Tabs, for the sign-in page.
    implementation("androidx.browser:browser:1.8.0")
}
