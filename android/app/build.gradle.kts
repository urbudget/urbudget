import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val requestedAbis = providers.gradleProperty("androidAbis").orElse("arm64-v8a,x86_64").get().split(',').map(String::trim)
require(requestedAbis.isNotEmpty() && requestedAbis.all { it in listOf("arm64-v8a", "x86_64") }) { "androidAbis supports arm64-v8a,x86_64." }
val uploadValues = listOf("URBUDGET_ANDROID_KEYSTORE", "URBUDGET_ANDROID_KEY_ALIAS", "URBUDGET_ANDROID_STORE_PASSWORD", "URBUDGET_ANDROID_KEY_PASSWORD").map { System.getenv(it)?.takeIf(String::isNotBlank) }
require(uploadValues.none { it != null } || uploadValues.all { it != null }) { "Set all four URBUDGET_ANDROID_* signing variables." }
// Instrumentation-only development Rust build enables the loopback API override. Release always stays optimized.
val rustDebug = providers.gradleProperty("rustDebug").orElse("false").map(String::toBoolean).get()
// The NDK the Rust engine is compiled with (buildRustEngine), which also strips it for packaging.
val ndkHome = System.getenv("ANDROID_NDK_HOME")?.takeIf(String::isNotBlank) ?: System.getenv("NDK_HOME")?.takeIf(String::isNotBlank)

android {
    namespace = "app.urbudget.nativeapp"
    compileSdk = 37
    // Without an NDK AGP packages the engine unstripped and can't take its symbols out for Play.
    // It only takes an NDK of the version it's told, so the version is that NDK's own.
    ndkHome?.let { home ->
        ndkPath = home
        ndkVersion = file(home).resolve("source.properties").readLines().first { it.startsWith("Pkg.Revision") }.substringAfter('=').trim()
    }
    defaultConfig {
        applicationId = "app.urbudget"
        minSdk = 26
        targetSdk = 37
        // Preserve Tauri Android's million-based version scheme for in-place upgrades.
        versionCode = providers.gradleProperty("versionCode").orElse("1000003").get().toInt()
        versionName = "1.0.3"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += requestedAbis }
        // Whether the engine is a debug build, which talks to URBUDGET_API's local server instead of
        // urbudget.app. A debug app with a release engine never counts itself (UsageCounts).
        buildConfigField("boolean", "RUST_DEBUG", rustDebug.toString())
    }
    buildFeatures { compose = true; buildConfig = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    if (uploadValues.all { it != null }) signingConfigs.create("upload") {
        storeFile = file(uploadValues[0]!!)
        keyAlias = uploadValues[1]; storePassword = uploadValues[2]; keyPassword = uploadValues[3]
    }
    buildTypes {
        // -PdebugSuffix=.syncqa makes a test copy that installs beside the sideloaded app, whose
        // budgets it then can't touch.
        debug { applicationIdSuffix = providers.gradleProperty("debugSuffix").orElse(".nativeapp").get() }
        release {
            if (uploadValues.all { it != null }) signingConfig = signingConfigs.getByName("upload")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // The engine ships stripped; its symbol table goes in the bundle, so Play shows native crashes with function names.
            ndk { debugSymbolLevel = "SYMBOL_TABLE" }
        }
        create("preview") {
            // Device evaluation must use release optimizations. Keep the debug identity and
            // certificate so installing this build preserves the sideloaded app's budgets.
            initWith(getByName("release"))
            applicationIdSuffix = ".nativeapp"
            signingConfig = signingConfigs.getByName("debug")
            isDebuggable = false
            matchingFallbacks += "release"
        }
    }
    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("generated/rustJniLibs").get().asFile)
    sourceSets["main"].kotlin.srcDir(rootProject.file("../shared/android/src/main/java"))
    sourceSets["test"].kotlin.srcDir(rootProject.file("../shared/android/src/test/java"))
    sourceSets["androidTest"].assets.srcDir(rootProject.file("../shared/fixtures"))
    sourceSets["test"].resources.srcDir(rootProject.file("../shared/fixtures"))
    packaging.resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    testOptions.unitTests.isReturnDefaultValues = true
}

kotlin { compilerOptions { jvmTarget.set(JvmTarget.JVM_17) } }

// Direct NDK compilation of the reusable Rust engine. No WebView, Node, or Tauri build.
val repositoryRoot = rootProject.projectDir.parentFile
val rustManifest = repositoryRoot.resolve("crates/urbudget-android/Cargo.toml")
val rustTarget = layout.buildDirectory.dir("rust-target")
// Inspect resolved tasks: aggregate names (assemble/build) and abbreviations can include release.
// This runs even when buildRustEngine is up to date, before a QA library can be packaged.
val rustAppProject = project
gradle.taskGraph.whenReady {
    require(!rustDebug || allTasks.none {
        it.project == rustAppProject && (it.name.contains("release", true) || it.name.contains("preview", true))
    }) { "rustDebug is only for isolated debug instrumentation." }
}
val nativeOutputs = layout.buildDirectory.dir("generated/rustJniLibs")
val buildRust = tasks.register("buildRustEngine") {
    group = "build"
    description = "Compile the desktop-shared Rust engine for Android ARM64 and x86_64."
    inputs.files(fileTree(repositoryRoot.resolve("crates")) { include("**/*.rs", "**/Cargo.toml", "**/Cargo.lock") })
    inputs.property("androidAbis", requestedAbis)
    inputs.property("rustDebug", rustDebug)
    outputs.dir(nativeOutputs)
    doLast {
        val ndk = ndkHome ?: throw GradleException("Set ANDROID_NDK_HOME or NDK_HOME to an installed Android NDK.")
        val host = when {
            System.getProperty("os.name").startsWith("Windows") -> "windows-x86_64"
            System.getProperty("os.name").startsWith("Mac") -> "darwin-x86_64"
            else -> "linux-x86_64"
        }
        val bin = file(ndk).resolve("toolchains/llvm/prebuilt/$host/bin")
        val windows = host.startsWith("windows")
        listOf("arm64-v8a" to "aarch64-linux-android", "x86_64" to "x86_64-linux-android").filter { it.first in requestedAbis }.forEach { (abi, target) ->
            val linker = bin.resolve("${target}26-clang" + if (windows) ".cmd" else "")
            check(linker.isFile) { "NDK compiler is missing: $linker" }
            val command = listOf("cargo", "build", "--locked") + (if (rustDebug) emptyList() else listOf("--release")) + listOf("--manifest-path", rustManifest.absolutePath, "--target", target, "--target-dir", rustTarget.get().asFile.absolutePath)
            val process = ProcessBuilder(command).directory(repositoryRoot).redirectErrorStream(true).apply {
                environment()["CARGO_TARGET_${target.uppercase().replace('-', '_')}_LINKER"] = linker.absolutePath
                environment()["CC_${target.replace('-', '_')}"] = linker.absolutePath
                environment()["AR_${target.replace('-', '_')}"] = bin.resolve("llvm-ar" + if (windows) ".exe" else "").absolutePath
            }.start()
            process.inputStream.bufferedReader().useLines { lines -> lines.forEach { logger.lifecycle(it) } }
            check(process.waitFor() == 0) { "Rust compilation failed for $abi. Install the Rust target with rustup target add $target." }
            copy {
                from(rustTarget.get().file("$target/${if (rustDebug) "debug" else "release"}/liburbudget_android.so"))
                into(nativeOutputs.get().dir(abi))
            }
        }
    }
}
tasks.named("preBuild").configure { dependsOn(buildRust) }

dependencies {
    implementation("com.android.billingclient:billing:9.1.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.core:core-ktx:1.16.0")
    // Custom Tabs: pages the services open show over the app, and their close button comes back to it.
    implementation("androidx.browser:browser:1.8.0")
    implementation("androidx.work:work-runtime-ktx:2.11.2")
    // Scanning the one-time code that adds a device to sync (ui/CodeScanner.kt): the camera through
    // CameraX, and ZXing to read the code from its frames on the phone. No scanner of Google's or
    // anyone's is called, so nothing the camera sees leaves the device.
    implementation("androidx.camera:camera-camera2:1.4.2")
    implementation("androidx.camera:camera-lifecycle:1.4.2")
    implementation("androidx.camera:camera-view:1.4.2")
    implementation("com.google.zxing:core:3.5.3")
    implementation(platform("androidx.compose:compose-bom:2025.08.00"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    // The fold: its posture and where its hinge runs, so layouts keep clear of it (ui/Adaptive.kt).
    implementation("androidx.window:window:1.5.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.10.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20250517")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation(platform("androidx.compose:compose-bom:2025.08.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    // Compose's older transitive Espresso reflects an InputManager method removed in API 37.
    // https://github.com/android/android-test/issues/2507
    androidTestImplementation("androidx.test.espresso:espresso-core:3.7.0")
}
