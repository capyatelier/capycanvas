plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}
val capyAbis = providers.gradleProperty("capyAbi").getOrElse("arm64-v8a,x86_64").split(",")

android {
    namespace = "art.capycanvas"
    compileSdk = 37
    buildToolsVersion = "37.0.0"
    ndkVersion = "29.0.14206865"
    defaultConfig {
        applicationId = "art.capycanvas"
        minSdk = 29
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk.abiFilters.addAll(capyAbis)
    }
    buildFeatures { compose = true; buildConfig = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs").get().asFile)
    sourceSets["main"].assets.srcDirs("../../layer-web/icons", "../../layer-web/brush-previews")
    sourceSets["main"].assets.srcDir(layout.buildDirectory.dir("generated/capy/assets").get().asFile)
    sourceSets["main"].res.srcDir(layout.buildDirectory.dir("generated/capy/res").get().asFile)
    packaging { jniLibs.useLegacyPackaging = false }
    testOptions { animationsDisabled = true }
}

val rustBuild by tasks.registering(Exec::class) {
    val out = layout.buildDirectory.dir("rustJniLibs").get().asFile
    workingDir = rootDir.resolve("../..")
    environment("ANDROID_NDK_HOME", "${System.getenv("ANDROID_HOME") ?: System.getProperty("user.home") + "/Android/Sdk"}/ndk/29.0.14206865")
    commandLine(listOf("cargo", "ndk") + capyAbis.flatMap { listOf("-t", it) } +
        listOf("--platform", "29", "-o", out.absolutePath, "build", "--release", "-p", "layer-android"))
    inputs.files(fileTree(rootDir.resolve("../../crates")) { include("**/*.rs", "**/*.wgsl", "**/*.pgm", "**/*.png", "**/Cargo.toml") })
    inputs.files(fileTree(rootDir.resolve("native")) { include("**/*.rs", "Cargo.toml") })
    inputs.files(rootDir.resolve("../../Cargo.lock"), rootDir.resolve("../../Cargo.toml"))
    inputs.property("abi", capyAbis)
    outputs.dir(out)
}
tasks.named("preBuild") { dependsOn(rustBuild) }

// Adapt the existing brand path to Android's maskable launcher format. No second
// artwork source or checked-in raster exports.
val generateBrand by tasks.registering {
    val source = rootDir.resolve("../layer-web/icons/layer-zen-looking-up-symbolic.svg")
    val output = layout.buildDirectory.dir("generated/capy/res").get().asFile
    inputs.file(source)
    outputs.dir(output)
    doLast {
        val svg = source.readText()
        val path = Regex("""<path[^>]*\sd="([^"]+)"""").find(svg)?.groupValues?.get(1)
            ?: error("Missing shared brand path")
        val viewBox = Regex("""viewBox="([^"]+)"""").find(svg)!!.groupValues[1].split(" ").map(String::toFloat)
        val scale = 200 / maxOf(viewBox[2], viewBox[3])
        val x = (324 - viewBox[2] * scale) / 2 - viewBox[0] * scale
        val y = (324 - viewBox[3] * scale) / 2 - viewBox[1] * scale
        output.resolve("drawable").mkdirs()
        output.resolve("drawable/capy_mark.xml").writeText("""
            <vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="108dp" android:height="108dp" android:viewportWidth="324" android:viewportHeight="324">
              <group android:translateX="$x" android:translateY="$y" android:scaleX="$scale" android:scaleY="$scale">
                <path android:fillColor="#f6f5f4" android:fillType="evenOdd" android:pathData="$path" />
              </group>
            </vector>
        """.trimIndent())
    }
}
tasks.named("preBuild") { dependsOn(generateBrand) }

// Development APKs retain the repository's licensing and branding notices.
val copyNotices by tasks.registering(Sync::class) {
    from(rootDir.resolve("../..")) {
        include("LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "BRANDING.md", "THIRD_PARTY_NOTICES.md")
    }
    into(layout.buildDirectory.dir("generated/capy/assets/licenses"))
}
tasks.named("preBuild") { dependsOn(copyNotices) }

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.08.00"))
    implementation("androidx.activity:activity-compose:1.12.4")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
    implementation("androidx.core:core-ktx:1.17.0")
    implementation("com.caverock:androidsvg-aar:1.4")
    androidTestImplementation(platform("androidx.compose:compose-bom:2026.08.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
