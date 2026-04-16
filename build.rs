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
    const DEFAULT_MAPLIBRE_VERSION: &str = "4.7.1";
    const PLACEHOLDER_MARKER: &str = "gs26-maplibre-placeholder";

    println!("cargo:rerun-if-env-changed=MAPLIBRE_GL_VERSION");

    let version =
        env::var("MAPLIBRE_GL_VERSION").unwrap_or_else(|_| DEFAULT_MAPLIBRE_VERSION.to_string());
    let vendor_dir = manifest_dir
        .join("static")
        .join("vendor")
        .join("maplibre-gl");
    let stamp = vendor_dir.join(".maplibre-gl-stamp");
    let stamp_contents = format!("version={version}\n");

    for asset in ["maplibre-gl.css", "maplibre-gl.js"] {
        println!(
            "cargo:rerun-if-changed={}",
            vendor_dir.join(asset).display()
        );
    }
    println!("cargo:rerun-if-changed={}", stamp.display());

    if let Err(e) = fs::create_dir_all(&vendor_dir) {
        log(format!("Failed to create MapLibre vendor dir {vendor_dir:?}: {e}").as_ref());
        return;
    }

    if let Err(e) = write_if_changed(&stamp, &stamp_contents) {
        log(format!("Failed to write MapLibre stamp {}: {e}", stamp.display()).as_ref());
    }

    for (filename, cdn_path) in [
        ("maplibre-gl.css", "dist/maplibre-gl.css"),
        ("maplibre-gl.js", "dist/maplibre-gl.js"),
    ] {
        if let Err(e) = download_vendor_asset(
            &vendor_dir,
            &stamp_contents,
            filename,
            &format!("https://unpkg.com/maplibre-gl@{version}/{cdn_path}"),
            PLACEHOLDER_MARKER,
            log,
        ) {
            log(format!("Failed to download MapLibre asset {filename}: {e}").as_ref());
        }
    }
}

fn download_vendor_asset(
    vendor_dir: &Path,
    expected_stamp: &str,
    filename: &str,
    url: &str,
    placeholder_marker: &str,
    log: impl Fn(&str),
) -> Result<(), Box<dyn std::error::Error>> {
    let out_path = vendor_dir.join(filename);
    let stamp_path = vendor_dir.join(".maplibre-gl-stamp");
    let existing_stamp = fs::read_to_string(&stamp_path).unwrap_or_default();

    let needs_download = match fs::read_to_string(&out_path) {
        Ok(existing) => {
            existing_stamp != expected_stamp
                || existing.contains(placeholder_marker)
                || existing.len() < 128
        }
        Err(_) => true,
    };

    if !needs_download {
        return Ok(());
    }

    log(format!("Downloading {url} -> {}", out_path.display()).as_ref());

    let resp = reqwest::blocking::get(url)?;
    if !resp.status().is_success() {
        return Err(format!("HTTP error: {}", resp.status()).into());
    }

    let bytes = resp.bytes()?;
    let mut file = fs::File::create(&out_path)?;
    file.write_all(&bytes)?;
    file.flush()?;

    Ok(())
}
