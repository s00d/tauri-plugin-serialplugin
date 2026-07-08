plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.serialport.usbtester"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.serialport.usbtester"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }

    kotlinOptions {
        jvmTarget = "11"
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

tasks.register<Exec>("buildRustJni") {
    workingDir = file("../rust")
    commandLine(
        "cargo", "ndk", "-t", "arm64-v8a", "-o", "../app/src/main/jniLibs", "build", "--release",
    )
}

tasks.named("preBuild") {
    dependsOn("buildRustJni")
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("com.google.android.material:material:1.11.0")
}
