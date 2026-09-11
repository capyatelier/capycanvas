#!/usr/bin/env python3
"""Deterministic Xcode project, without a global project-generator dependency."""
import hashlib
import json
from pathlib import Path

APP = Path(__file__).resolve().parents[1]
PROJECT = APP / "CapyCanvas.xcodeproj"
objects = {}

def ident(name):
    return hashlib.sha256(name.encode()).hexdigest()[:24].upper()

def obj(identity, isa, **values):
    key = ident(identity)
    objects[key] = {"isa": isa, **values}
    return key

def plist(value, depth=0):
    tab = "\t" * depth
    if isinstance(value, dict):
        return "{\n" + "".join(f"{tab}\t{json.dumps(k)} = {plist(v, depth+1)};\n" for k, v in value.items()) + tab + "}"
    if isinstance(value, list):
        return "(\n" + "".join(f"{tab}\t{plist(v, depth+1)},\n" for v in value) + tab + ")"
    return json.dumps(str(value))

def configs(name, settings):
    refs = []
    for config in ["Debug", "Release"]:
        values = dict(settings)
        values.update({"CAPY_RUST_PROFILE": "debug" if config == "Debug" else "release",
            "SWIFT_OPTIMIZATION_LEVEL": "-Onone" if config == "Debug" else "-O",
            "SWIFT_ACTIVE_COMPILATION_CONDITIONS": "DEBUG" if config == "Debug" else "",
            "DEBUG_INFORMATION_FORMAT": "dwarf" if config == "Debug" else "dwarf-with-dsym"})
        refs.append(obj(name + config, "XCBuildConfiguration", name=config, buildSettings=values))
    return obj(name + "configs", "XCConfigurationList", buildConfigurations=refs,
        defaultConfigurationIsVisible=0, defaultConfigurationName="Debug")

sources = sorted(APP.glob("Shared/**/*.swift")) + sorted(APP.glob("iOS/**/*.swift")) + sorted(APP.glob("macOS/**/*.swift"))
refs = {}
for path in sources:
    name = str(path.relative_to(APP))
    refs[name] = obj(name, "PBXFileReference", lastKnownFileType="sourcecode.swift", path=name, sourceTree="<group>")
for name, kind in [("Generated/SharedAssets.xcassets", "folder.assetcatalog"), ("Generated/filters", "folder"), ("Generated/licenses", "folder")]:
    refs[name] = obj(name, "PBXFileReference", lastKnownFileType=kind, path=name, sourceTree="<group>")

targets, products = [], []
for platform, scheme in [("iOS", "CapyCanvas-iPad"), ("macOS", "CapyCanvas-Mac")]:
    product = obj(scheme + "product", "PBXFileReference", explicitFileType="wrapper.application", path=scheme + ".app", sourceTree="BUILT_PRODUCTS_DIR")
    products.append(product)
    builds, resources = [], []
    for name, ref in refs.items():
        if (name.startswith("Shared/") or name.startswith(platform + "/")) and "/Tests/" not in name:
            builds.append(obj(scheme + name, "PBXBuildFile", fileRef=ref))
        elif name.startswith("Generated/"):
            resources.append(obj(scheme + name, "PBXBuildFile", fileRef=ref))
    phases = [obj(scheme + "rust", "PBXShellScriptBuildPhase", buildActionMask=2147483647,
        files=[], inputPaths=[], outputPaths=[], runOnlyForDeploymentPostprocessing=0,
        alwaysOutOfDate=1, name="Build shared Rust engine", shellPath="/bin/bash",
        shellScript='/bin/bash "$SRCROOT/scripts/rust.sh"\n'),
        obj(scheme + "sources", "PBXSourcesBuildPhase", buildActionMask=2147483647, files=builds, runOnlyForDeploymentPostprocessing=0),
        obj(scheme + "resources", "PBXResourcesBuildPhase", buildActionMask=2147483647, files=resources, runOnlyForDeploymentPostprocessing=0)]
    settings = {
        "PRODUCT_NAME": scheme, "PRODUCT_BUNDLE_IDENTIFIER": "art.capycanvas.apple." + ("ipad" if platform == "iOS" else "mac"),
        "CODE_SIGN_STYLE": "Automatic", "SWIFT_VERSION": "5.0", "CLANG_ENABLE_MODULES": "YES",
        "ENABLE_USER_SCRIPT_SANDBOXING": "NO", "SWIFT_OBJC_BRIDGING_HEADER": "$(SRCROOT)/native/include/CapyApple.h",
        "ASSETCATALOG_COMPILER_GENERATE_ASSET_SYMBOLS": "NO",
        "LIBRARY_SEARCH_PATHS": ["$(inherited)", "$(SRCROOT)/../../target/$(CAPY_RUST_TARGET)/$(CAPY_RUST_PROFILE)"],
        "OTHER_LDFLAGS": ["$(inherited)", "-llayer_apple", "-lc++", "-framework", "Metal", "-framework", "QuartzCore", "-framework", "Security"],
        "ARCHS": "arm64", "ONLY_ACTIVE_ARCH": "YES",
        "CAPY_RUST_TARGET": "aarch64-apple-ios" if platform == "iOS" else "aarch64-apple-darwin",
    }
    if platform == "iOS":
        settings.update({"SDKROOT": "iphoneos", "SUPPORTED_PLATFORMS": "iphoneos iphonesimulator",
            "CAPY_RUST_TARGET[sdk=iphonesimulator*]": "aarch64-apple-ios-sim",
            "TARGETED_DEVICE_FAMILY": "2", "IPHONEOS_DEPLOYMENT_TARGET": "18.0",
            "INFOPLIST_FILE": "iOS/App/Info.plist", "SUPPORTS_MACCATALYST": "NO"})
    else:
        settings.update({"SDKROOT": "macosx", "SUPPORTED_PLATFORMS": "macosx", "MACOSX_DEPLOYMENT_TARGET": "15.0", "GENERATE_INFOPLIST_FILE": "YES"})
    target = obj(scheme, "PBXNativeTarget", buildConfigurationList=configs(scheme, settings),
        buildPhases=phases, buildRules=[], dependencies=[], name=scheme, productName=scheme,
        productReference=product, productType="com.apple.product-type.application")
    targets.append(target)
    test_action = ""
    if platform == "iOS":
        test_name = scheme + "Tests"
        test_product = obj(test_name + "product", "PBXFileReference", explicitFileType="wrapper.cfbundle", path=test_name + ".xctest", sourceTree="BUILT_PRODUCTS_DIR")
        products.append(test_product)
        test_builds = [obj(test_name + name, "PBXBuildFile", fileRef=ref) for name, ref in refs.items() if name.startswith("iOS/Tests/")]
        test_phase = obj(test_name + "sources", "PBXSourcesBuildPhase", buildActionMask=2147483647, files=test_builds, runOnlyForDeploymentPostprocessing=0)
        proxy = obj(test_name + "proxy", "PBXContainerItemProxy", containerPortal=ident("project"), proxyType=1, remoteGlobalIDString=target, remoteInfo=scheme)
        dependency = obj(test_name + "dependency", "PBXTargetDependency", target=target, targetProxy=proxy)
        test_target = obj(test_name, "PBXNativeTarget", buildConfigurationList=configs(test_name, {
            "PRODUCT_NAME": test_name, "PRODUCT_BUNDLE_IDENTIFIER": "art.capycanvas.apple.ipad.tests",
            "CODE_SIGN_STYLE": "Automatic", "SWIFT_VERSION": "5.0", "SDKROOT": "iphoneos",
            "SUPPORTED_PLATFORMS": "iphoneos iphonesimulator", "TARGETED_DEVICE_FAMILY": "2",
            "IPHONEOS_DEPLOYMENT_TARGET": "18.0", "GENERATE_INFOPLIST_FILE": "YES",
            "TEST_TARGET_NAME": scheme, "ARCHS": "arm64", "CLANG_ENABLE_MODULES": "YES",
        }), buildPhases=[test_phase], buildRules=[], dependencies=[dependency], name=test_name,
            productName=test_name, productReference=test_product, productType="com.apple.product-type.bundle.ui-testing")
        targets.append(test_target)
        test_action = f'''<TestAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" shouldUseLaunchSchemeArgsEnv="YES"><Testables><TestableReference skipped="NO"><BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="{test_target}" BuildableName="{test_name}.xctest" BlueprintName="{test_name}" ReferencedContainer="container:CapyCanvas.xcodeproj"/></TestableReference></Testables></TestAction>'''
    scheme_dir = PROJECT / "xcshareddata/xcschemes"
    scheme_dir.mkdir(parents=True, exist_ok=True)
    ref = f'<BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="{target}" BuildableName="{scheme}.app" BlueprintName="{scheme}" ReferencedContainer="container:CapyCanvas.xcodeproj"/>'
    (scheme_dir / f"{scheme}.xcscheme").write_text(f'''<?xml version="1.0" encoding="UTF-8"?>
<Scheme LastUpgradeVersion="2660" version="1.3">
<BuildAction parallelizeBuildables="YES" buildImplicitDependencies="YES"><BuildActionEntries><BuildActionEntry buildForTesting="YES" buildForRunning="YES" buildForProfiling="YES" buildForArchiving="YES" buildForAnalyzing="YES">{ref}</BuildActionEntry></BuildActionEntries></BuildAction>
{test_action}
<LaunchAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" launchStyle="0" useCustomWorkingDirectory="NO" ignoresPersistentStateOnLaunch="NO" debugDocumentVersioning="YES" allowLocationSimulation="YES"><BuildableProductRunnable runnableDebuggingMode="0">{ref}</BuildableProductRunnable></LaunchAction>
<ProfileAction buildConfiguration="Release" shouldUseLaunchSchemeArgsEnv="YES" savedToolIdentifier="" useCustomWorkingDirectory="NO" debugDocumentVersioning="YES"><BuildableProductRunnable runnableDebuggingMode="0">{ref}</BuildableProductRunnable></ProfileAction>
<AnalyzeAction buildConfiguration="Debug"/><ArchiveAction buildConfiguration="Release" revealArchiveInOrganizer="YES"/>
</Scheme>
''')

product_group = obj("products", "PBXGroup", name="Products", children=products, sourceTree="<group>")
main_group = obj("main", "PBXGroup", children=list(refs.values()) + [product_group], sourceTree="<group>")
root = obj("project", "PBXProject", attributes={"LastUpgradeCheck": "2660", "BuildIndependentTargetsInParallel": "YES"},
    buildConfigurationList=configs("project", {}), compatibilityVersion="Xcode 14.0", developmentRegion="en",
    knownRegions=["en", "Base"], mainGroup=main_group, productRefGroup=product_group, projectDirPath="", projectRoot="", targets=targets)
PROJECT.mkdir(exist_ok=True)
(PROJECT / "project.pbxproj").write_text("// !$*UTF8*$!\n" + plist({"archiveVersion": 1, "classes": {}, "objectVersion": 56, "objects": objects, "rootObject": root}) + "\n")
