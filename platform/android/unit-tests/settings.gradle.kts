// JVM unit tests for the phone app's pure Android-side logic
// (../kotlin/com/ubseds/gs26/ProximityPolicy.kt). The app itself is built by
// build.py from the dx-generated project; this only compiles and tests the
// files listed in build.gradle.kts. Run (offline once the Wear build cached
// its dependencies):
//   ../../../watch/wear/gradlew -p . test
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "gs26-android-unit-tests"
