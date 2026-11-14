import org.gradle.api.file.DirectoryProperty
import org.gradle.api.tasks.OutputDirectory

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}
val capyAbis = providers.gradleProperty("capyAbi").getOrElse("arm64-v8a,x86_64").split(",")
val capyRustProfile = providers.gradleProperty("capyRustProfile")
    .orElse(providers.environmentVariable("CAPY_RUST_PROFILE"))
val capyApplicationId = providers.gradleProperty("capyApplicationId")
val capyDevelopmentSuffix = ".dev".takeUnless { capyApplicationId.isPresent }
val capySourceRevision = providers.exec { commandLine("sh", rootDir.resolve("../../tools/build/source-revision.sh").path) }
    .standardOutput.asText.map(String::trim)
val capyVersion = Regex("^version = \"(\\d+)\\.(\\d+)\\.(\\d+)\"$", RegexOption.MULTILINE)
    .find(providers.fileContents(rootProject.layout.projectDirectory.file("../../Cargo.toml")).asText.get())!!
    .groupValues.drop(1).map(String::toInt)

abstract class RustBuild : Exec() {
    @get:OutputDirectory
    abstract val jniDirectory: DirectoryProperty
}

android {
    namespace = "art.capycanvas"
    compileSdk = 37
    buildToolsVersion = "37.0.0"
    ndkVersion = "29.0.14206865"
    defaultConfig {
        applicationId = capyApplicationId.getOrElse("art.capycanvas")
        manifestPlaceholders["capyAppLabel"] = providers.gradleProperty("capyAppLabel").getOrElse("@string/capy_app_name")
        minSdk = 29
        targetSdk = 37
        versionCode = capyVersion[0] * 1_000_000 + capyVersion[1] * 1_000 + capyVersion[2]
        versionName = capyVersion.joinToString(".")
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        testProguardFiles("test-proguard-rules.pro")
        buildConfigField("boolean", "WORKSPACE_BENCHMARK", "false")
        ndk.abiFilters.addAll(capyAbis)
    }
    buildFeatures { compose = true; buildConfig = true }
    buildTypes {
        getByName("debug") {
            applicationIdSuffix = capyDevelopmentSuffix
        }
        getByName("release") {
            applicationIdSuffix = ".editor".takeUnless { capyApplicationId.isPresent }
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            ndk.debugSymbolLevel = "FULL"
            providers.environmentVariable("CAPY_UPLOAD_KEYSTORE").orNull?.let { keystore ->
                signingConfig = signingConfigs.create("upload") {
                    storeFile = file(keystore)
                    storePassword = providers.environmentVariable("CAPY_UPLOAD_KEYSTORE_PASSWORD").get()
                    keyAlias = "upload"
                    keyPassword = storePassword
                }
            }
        }
        create("benchmark") {
            initWith(getByName("release"))
            applicationIdSuffix = capyDevelopmentSuffix
            signingConfig = signingConfigs.getByName("debug")
            isDebuggable = false
            // White-box regression APKs keep their existing unminified ABI.
            // The self-instrumenting runner also tests full release optimization.
            isMinifyEnabled = providers.gradleProperty("capyOptimize").map { it != "false" }.getOrElse(false)
            isShrinkResources = isMinifyEnabled
            buildConfigField("boolean", "WORKSPACE_BENCHMARK", "true")
        }
    }
    testBuildType = if (providers.gradleProperty("capyBenchmark").map { it != "false" }.getOrElse(false)) "benchmark" else "debug"
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    sourceSets["main"].assets.srcDirs("../../layer-web/icons", "../../layer-web/brush-previews", "../../layer-web/filter-previews")
    sourceSets["main"].assets.srcDir(layout.buildDirectory.dir("generated/capy/assets").get().asFile)
    sourceSets["main"].res.srcDir(layout.buildDirectory.dir("generated/capy/res").get().asFile)
    sourceSets["androidTest"].assets.srcDirs("../../../crates/layer-workspace/tests/compat/1.0.15", "../../../crates/layer-workspace/tests/fixtures")
    sourceSets["benchmark"].kotlin.directories.add("src/workspaceBenchmark/java")
    sourceSets.maybeCreate("androidTestDebug").kotlin.directories.add("src/workspaceBenchmark/java")
    testOptions { animationsDisabled = true }
}

androidComponents.onVariants { variant ->
    val profile = capyRustProfile.getOrElse(if (variant.buildType == "debug") "dev-perf" else "release")
    val rustBuild = tasks.register<RustBuild>("rustBuild${variant.name.replaceFirstChar { it.uppercase() }}") {
        jniDirectory.set(layout.buildDirectory.dir("rustJniLibs/${variant.name}"))
        workingDir = rootDir.resolve("../..")
        environment("ANDROID_NDK_HOME", "${System.getenv("ANDROID_HOME") ?: System.getProperty("user.home") + "/Android/Sdk"}/ndk/29.0.14206865")
        environment("CAPY_SOURCE_REVISION", capySourceRevision.get())
        commandLine(listOf("cargo", "ndk") + capyAbis.flatMap { listOf("-t", it) } +
            listOf("--platform", "29", "-o", jniDirectory.get().asFile.absolutePath,
                "build", "--locked", "--profile", profile, "-p", "layer-android-link"))
        inputs.files(fileTree(rootDir.resolve("../../crates")) { include("**/*.rs", "**/*.wgsl", "**/*.pgm", "**/*.png", "**/*.bin", "**/Cargo.toml") })
        inputs.files(fileTree(rootDir.resolve("../../vendor")) { include("**/*.rs", "**/*.wgsl", "**/Cargo.toml") })
        inputs.files(fileTree(rootDir.resolve("native")) { include("**/*.rs", "**/Cargo.toml") })
        inputs.files(fileTree(rootDir.resolve("../../assets/filters")))
        inputs.files(fileTree(rootDir.resolve("../../assets/locales")) { include("**/*.ftl") })
        inputs.files(rootDir.resolve("../../Cargo.lock"), rootDir.resolve("../../Cargo.toml"), rootDir.resolve("../../.cargo/config.toml"))
        inputs.property("abi", capyAbis)
        inputs.property("profile", profile)
        inputs.property("sourceRevision", capySourceRevision)
        inputs.property("publicRelease", providers.environmentVariable("CAPY_PUBLIC_RELEASE").orElse(""))
        inputs.property("rustVersion", providers.exec { commandLine("rustc", "--version") }.standardOutput.asText)
    }
    variant.sources.jniLibs?.addGeneratedSourceDirectory(rustBuild) { it.jniDirectory }
}

val generateAppNames by tasks.registering(Exec::class) {
    val script = rootDir.resolve("../../tools/build/app_names.py")
    val output = layout.buildDirectory.dir("generated/capy/appNames").get().asFile
    inputs.file(script)
    inputs.file(rootDir.resolve("../../crates/layer-ui/src/localization_languages.rs"))
    inputs.files(fileTree(rootDir.resolve("../../assets/locales")) { include("*/common.ftl") })
    outputs.dir(output)
    commandLine("python3", script, "android", output)
}
android.sourceSets["main"].res.srcDir(layout.buildDirectory.dir("generated/capy/appNames").get().asFile)
tasks.named("preBuild") { dependsOn(generateAppNames) }

val launchMarksFile = layout.buildDirectory.file("generated/capy/launchMarks.txt").get().asFile
val launchMarks by tasks.registering(Exec::class) {
    workingDir = rootDir.resolve("../..")
    inputs.files(fileTree(rootDir.resolve("../../crates/layer-ui")) { include("**/*.rs", "Cargo.toml") })
    inputs.files(rootDir.resolve("../../Cargo.lock"))
    outputs.file(launchMarksFile)
    commandLine("sh", "-c", "cargo run --locked --quiet -p layer-ui --example launch_marks > \"\$1\"", "sh", launchMarksFile.absolutePath)
}

// Adapt the existing brand paths to Android's maskable launcher and splash
// formats. No second artwork source or checked-in raster exports.
val generateBrand by tasks.registering {
    val icons = rootDir.resolve("../layer-web/icons")
    val output = layout.buildDirectory.dir("generated/capy/res").get().asFile
    inputs.files(launchMarks)
    inputs.files(fileTree(icons) { include("layer-zen-*-symbolic.svg") })
    outputs.dir(output)
    doLast {
        fun artwork(icon: String): Pair<String, List<Float>> {
            val svg = icons.resolve("layer-$icon-symbolic.svg").readText()
            val path = Regex("""<path[^>]*\sd="([^"]+)"""").find(svg)?.groupValues?.get(1)
                ?: error("Missing shared $icon path")
            return path to Regex("""viewBox="([^"]+)"""").find(svg)!!.groupValues[1].split(" ").map(String::toFloat)
        }
        output.deleteRecursively()
        for (directory in listOf("drawable", "values", "mipmap-anydpi", "mipmap-anydpi-v33")) output.resolve(directory).mkdirs()
        val poses = launchMarksFile.readLines().filter(String::isNotBlank).map { it.split(" ") }
        for ((icon, centerX, centerY, radius) in poses) {
            val name = icon.replace('-', '_')
            val (pose, box) = artwork(icon)
            // The launcher displays the center 72dp of the 108dp adaptive layer.
            // Match the favicon's 440/512 artwork within that visible square.
            val scale = (216f * 440f / 512f) / maxOf(box[2], box[3])
            val x = (324 - box[2] * scale) / 2 - box[0] * scale
            val y = (324 - box[3] * scale) / 2 - box[1] * scale
            output.resolve("drawable/capy_mark_$name.xml").writeText("""
                <vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="108dp" android:height="108dp" android:viewportWidth="324" android:viewportHeight="324">
                  <group android:translateX="$x" android:translateY="$y" android:scaleX="$scale" android:scaleY="$scale">
                    <path android:fillColor="#f6f5f4" android:fillType="evenOdd" android:pathData="$pose" />
                  </group>
                </vector>
            """.trimIndent())
            for ((directory, monochrome) in listOf("mipmap-anydpi" to "", "mipmap-anydpi-v33" to """<monochrome android:drawable="@drawable/capy_mark_$name" />"""))
                output.resolve("$directory/capy_launcher_$name.xml").writeText("""
                    <adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
                      <background android:drawable="@color/capy_brand_background" />
                      <foreground android:drawable="@drawable/capy_mark_$name" />
                      $monochrome
                    </adaptive-icon>
                """.trimIndent())
            output.resolve("drawable/capy_launch_mark_$name.xml").writeText("""
                <vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="96dp" android:height="96dp" android:viewportWidth="${box[2]}" android:viewportHeight="${box[3]}">
                  <group android:translateX="${-box[0]}" android:translateY="${-box[1]}">
                    <path android:fillColor="@color/capy_splash_foreground" android:fillType="evenOdd" android:pathData="$pose" />
                  </group>
                </vector>
            """.trimIndent())
            val splashScale = 95f / radius.toFloat()
            output.resolve("drawable/capy_splash_mark_$name.xml").writeText("""
                <vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="288dp" android:height="288dp" android:viewportWidth="288" android:viewportHeight="288">
                  <group android:translateX="${144f - centerX.toFloat() * splashScale}" android:translateY="${144f - centerY.toFloat() * splashScale}" android:scaleX="$splashScale" android:scaleY="$splashScale">
                    <path android:fillColor="@color/capy_splash_foreground" android:fillType="evenOdd" android:pathData="$pose" />
                  </group>
                </vector>
            """.trimIndent())
        }
        val names = poses.map { it[0].replace('-', '_') }
        output.resolve("values/launch_marks.xml").writeText(buildString {
            appendLine("""<?xml version="1.0" encoding="utf-8"?>""")
            appendLine("""<resources xmlns:tools="http://schemas.android.com/tools">""")
            for (name in names) appendLine("""    <style name="Theme.CapyCanvas.$name" parent="Theme.CapyCanvas"><item name="android:windowSplashScreenAnimatedIcon" tools:targetApi="31">@drawable/capy_splash_mark_$name</item></style>""")
            appendLine("""    <string-array name="capy_launch_icons" translatable="false">${poses.joinToString("") { "<item>${it[0]}</item>" }}</string-array>""")
            appendLine("""    <array name="capy_launch_marks">${names.joinToString("") { "<item>@drawable/capy_launch_mark_$it</item>" }}</array>""")
            appendLine("""    <array name="capy_launch_splash_themes">${names.joinToString("") { "<item>@style/Theme.CapyCanvas.$it</item>" }}</array>""")
            appendLine("</resources>")
        })
    }
}
tasks.named("preBuild") { dependsOn(generateBrand) }

val generateRustNotices by tasks.registering(Exec::class) {
    val out = layout.buildDirectory.dir("generated/capy/rustNotices").get().asFile
    workingDir = rootDir.resolve("../..")
    commandLine(listOf("node", rootDir.resolve("package-notices.mjs").absolutePath, out.absolutePath) + capyAbis)
    inputs.file(rootDir.resolve("package-notices.mjs"))
    inputs.files(rootDir.resolve("../../Cargo.lock"), rootDir.resolve("../../Cargo.toml"), rootDir.resolve("native/Cargo.toml"))
    inputs.files(fileTree(rootDir.resolve("../../crates")) { include("**/Cargo.toml") })
    inputs.files(fileTree(rootDir.resolve("../../vendor")) { include("**/Cargo.toml", "**/LICENSE*", "**/COPYING*") })
    inputs.files(fileTree(rootDir.resolve("../../tools/build")) { include("dependency-notices.mjs", "about.toml", "licenses/**") })
    inputs.property("abi", capyAbis)
    inputs.property("rustVersion", providers.exec { commandLine("rustc", "--version") }.standardOutput.asText)
    outputs.dir(out)
}

// Every APK retains the original Rust dependency, toolchain and project notices.
val copyNotices by tasks.registering(Sync::class) {
    dependsOn(generateRustNotices)
    from(layout.buildDirectory.dir("generated/capy/rustNotices"))
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
    "benchmarkImplementation"("androidx.compose.ui:ui-test-manifest")
}
