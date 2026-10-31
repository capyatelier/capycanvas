buildscript {
    repositories { google() }
    dependencies { classpath("com.android.tools:r8:9.6.0-alpha09") }
}

plugins {
    id("com.android.application") version "9.3.2" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.10" apply false
}
