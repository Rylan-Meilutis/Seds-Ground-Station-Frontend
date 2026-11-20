import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    // The same plugin set as watch/wear, so one dependency cache serves both.
    id("com.android.application") version "8.13.2" apply false
    kotlin("jvm") version "2.2.21"
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

sourceSets {
    main {
        kotlin {
            // Only Android-free files: the rest need the Android SDK.
            setSrcDirs(listOf("../kotlin"))
            include("com/ubseds/gs26/ProximityPolicy.kt")
        }
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}

tasks.test {
    testLogging { events("failed"); showStandardStreams = false }
}
