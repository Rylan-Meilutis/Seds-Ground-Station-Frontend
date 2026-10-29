use std::process::Output;
use std::{env, fs, io::Write, path::Path, path::PathBuf, process::Command};

fn compile_theme_catalog(manifest_dir: &Path) {
    let src = manifest_dir
        .join("assets")
        .join("themes")
        .join("presets.json");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = fs::read_to_string(&src)
        .unwrap_or_else(|e| panic!("failed to read theme catalog {}: {e}", src.display()));

    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("theme catalog JSON is invalid ({}): {e}", src.display()));
    let presets = parsed
        .get("presets")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("theme catalog must contain a top-level 'presets' array"));
    if presets.is_empty() {
        panic!("theme catalog must contain at least one preset");
    }
    for preset in presets {
        let id = preset
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("each theme preset must contain a string 'id'"));
        if id.trim().is_empty() {
            panic!("theme preset ids must not be empty");
        }
        if preset.get("theme").is_none() {
            panic!("theme preset '{id}' is missing its 'theme' object");
        }
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    write_if_changed(&out_dir.join("theme_presets.json"), &raw)
        .unwrap_or_else(|e| panic!("failed to write compiled theme catalog: {e}"));
}

fn build_apple_objc(manifest_dir: &Path, target: &str) {
    if !target.contains("apple-") {
        return;
    }

    let src = manifest_dir.join("assets/LocationShim.m");
    println!("cargo:rerun-if-changed={}", src.display());
    if !src.exists() {
        panic!("ObjC shim not found: {}", src.display());
    }

    let (sdk, clang_target, out_subdir) = if target.contains("apple-ios-sim") {
        (
            "iphonesimulator",
            "arm64-apple-ios13.0-simulator",
            "ios-sim",
        )
    } else if target.contains("apple-ios") {
        ("iphoneos", "arm64-apple-ios13.0", "ios")
    } else if target.contains("apple-darwin") {
        if target.starts_with("x86_64") {
            ("macosx", "x86_64-apple-macosx10.15", "macos")
        } else {
            ("macosx", "arm64-apple-macosx10.15", "macos")
        }
    } else {
        return;
    };

    let sdk_path = {
        let out = Command::new("xcrun")
            .arg("--sdk")
            .arg(sdk)
            .arg("--show-sdk-path")
            .output()
            .expect("failed to run xcrun --show-sdk-path");

        if !out.status.success() {
            panic!(
                "xcrun --show-sdk-path failed for sdk={sdk}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }

        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };

    let profile = env::var("PROFILE").unwrap();
    let out_dir = manifest_dir
        .join("objc-build")
        .join(profile)
        .join("gs26location")
        .join(out_subdir);
    fs::create_dir_all(&out_dir).unwrap();

    let obj = out_dir.join("LocationShim.o");
    let lib = out_dir.join("libgs26location.a");
    let mut cmd = Command::new("xcrun");
    cmd.arg("--sdk")
        .arg(sdk)
        .arg("clang")
        .arg("-target")
        .arg(clang_target)
        .arg("-isysroot")
        .arg(&sdk_path)
        .arg("-fobjc-arc")
        .arg("-c")
        .arg(&src)
        .arg("-o")
        .arg(&obj);
    // compile .m -> .o
    run(cmd);
    let mut cmd = Command::new("xcrun");

    cmd.arg("libtool")
        .arg("-static")
        .arg("-o")
        .arg(&lib)
        .arg(&obj);
    // archive -> .a
    run(cmd);

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=gs26location");

    // frameworks used
    println!("cargo:rustc-link-lib=framework=CoreLocation");
    println!("cargo:rustc-link-lib=framework=CoreMotion");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=Network");
    println!("cargo:rustc-link-lib=objc");
}

fn build_windows_resources(manifest_dir: &Path, target: &str) {
    if !target.contains("windows") {
        return;
    }
    let host = env::var("HOST").unwrap_or_default();

    let icon = manifest_dir.join("assets").join("icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if !icon.exists() {
        panic!("Windows icon not found: {}", icon.display());
    }

    if !host.contains("windows") {
        println!(
            "cargo:warning=Skipping Windows resource compilation while cross-checking from non-Windows host ({host})"
        );
        return;
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon(icon.to_string_lossy().as_ref());
    if let Err(err) = res.compile() {
        panic!("failed to compile Windows resources: {err}");
    }
}

fn run(mut cmd: Command) {
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    let out: Output = cmd.output().unwrap_or_else(|e| {
        panic!("failed to spawn: {program} {args:?}\nerror: {e}");
    });

    if !out.status.success() {
        panic!(
            "command failed: {program} {args:?}\n\
             status: {}\n\
             stdout:\n{}\n\
             stderr:\n{}\n",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

fn log(msg: &str) {
    let log_file_path = env::var("CARGO_MANIFEST_DIR").unwrap().to_string() + "/build.log";
    let mut log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .unwrap();

    log_file.write_all(msg.as_ref()).unwrap();
}

fn write_if_changed(path: &Path, contents: &str) -> std::io::Result<()> {
    match fs::read_to_string(path) {
        Ok(existing) if existing == contents => Ok(()),
        Ok(_) | Err(_) => fs::write(path, contents),
    }
}

fn main() {
    let target = env::var("TARGET").unwrap();
    use fs;
    let log_file_path = env::var("CARGO_MANIFEST_DIR").unwrap().to_string() + "/build.log";
    let _ = fs::remove_file(&log_file_path);

    log(format!("target: {}", target).as_ref());

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    build_apple_objc(&manifest_dir, &target);
    build_windows_resources(&manifest_dir, &target);
    compile_theme_catalog(&manifest_dir);
    // Re-run if this file changes
    println!("cargo:rerun-if-changed=build.rs");

    stage_maplibre_assets(&manifest_dir);
}

fn stage_maplibre_assets(manifest_dir: &Path) {
    for name in [
        "MAPLIBRE_GL_VERSION",
        "MAPLIBRE_GL_BASE_URL",
        "GS_BROWSER_ASSETS_CACHE",
        "GS_PYTHON",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let script = manifest_dir.join("scripts/browser_assets.py");
    println!("cargo:rerun-if-changed={}", script.display());
    for name in [
        "maplibre-gl.css",
        "maplibre-gl.js",
        ".maplibre-gl-assets.json",
        ".maplibre-gl-stamp",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            manifest_dir
                .join("static/vendor/maplibre-gl")
                .join(name)
                .display()
        );
    }
    let candidates = env::var_os("GS_PYTHON")
        .map(|python| vec![python])
        .unwrap_or_else(|| vec!["python3".into(), "python".into(), "py".into()]);
    for python in candidates {
        let mut probe = std::process::Command::new(&python);
        if python == "py" {
            probe.arg("-3");
        }
        match probe
            .args([
                "-c",
                "import sys; sys.exit(0 if sys.version_info >= (3, 10) else 1)",
            ])
            .output()
        {
            Ok(output) if output.status.success() => {}
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("Unable to find Python3.10+ for browser assets: {error}"),
        }
        let mut command = Command::new(&python);
        if python == "py" {
            command.arg("-3");
        }
        let result = command
            .arg(&script)
            .arg("--root")
            .arg(manifest_dir)
            .output();
        match result {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("Unable to prepare frontend browser assets: {error}"),
            Ok(output) if output.status.success() => return,
            Ok(output) => panic!(
                "Frontend browser asset preparation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }
    panic!(
        "Python3.10+ is required to prepare frontend browser assets; set GS_PYTHON to its executable"
    );
}
