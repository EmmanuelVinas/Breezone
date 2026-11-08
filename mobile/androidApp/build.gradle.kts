import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.androidApplication)
    alias(libs.plugins.composeCompiler)
}

dependencies {
    implementation(projects.shared)
    implementation(libs.androidx.activity.compose)
    implementation(libs.compose.foundation)
    implementation(libs.koin.android)
}

// Release signing (never committed): mobile/docs/stores/android/keystore.properties (see the
// README of that folder), or mobile/keystore.properties.
//   storeFile=keys/breezone-upload.jks   (relative to the properties file, or absolute)
//   storePassword=…
//   keyAlias=upload
//   keyPassword=…
val keystoreFile = listOf(rootProject.file("docs/stores/android/keystore.properties"), rootProject.file("keystore.properties"))
    .firstOrNull { it.exists() }
val keystoreProperties = Properties().apply {
    keystoreFile?.inputStream()?.use { load(it) }
}

android {
    namespace = "com.breezone.app"
    compileSdk {
        version = release(libs.versions.android.compileSdk.get().toInt()) {
            minorApiLevel = libs.versions.android.compileSdkMinor.get().toInt()
        }
    }

    defaultConfig {
        applicationId = "com.breezone.app"
        minSdk = libs.versions.android.minSdk.get().toInt()
        targetSdk = libs.versions.android.targetSdk.get().toInt()
        versionCode = (findProperty("breezone.versionCode") as String).toInt()
        versionName = findProperty("breezone.versionName") as String
    }
    signingConfigs {
        if (keystoreProperties.isNotEmpty()) {
            create("release") {
                storeFile = keystoreFile!!.parentFile.resolve(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }
    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_11
    }
}
