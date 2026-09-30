// SPDX-License-Identifier: Apache-2.0 OR MIT
//! `cargo xtask datagen`: download the vanilla server, run its data generator and
//! generate the protocol crate's tables from the reports.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::{Result, codegen};

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

pub fn run(version: Option<&str>) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()?;
    let cache = root.join("target/xtask/datagen");
    fs::create_dir_all(&cache)?;

    let manifest = fetch_json(MANIFEST, &cache.join("version_manifest_v2.json"))?;
    let version = match version {
        Some(v) => v.to_owned(),
        None => str_at(&manifest, &["latest", "release"])?.to_owned(),
    };
    let entry = manifest["versions"]
        .as_array()
        .and_then(|vs| vs.iter().find(|v| v["id"] == version.as_str()))
        .ok_or_else(|| format!("version {version} is not in the Mojang version manifest"))?;

    let dir = cache.join(&version);
    fs::create_dir_all(&dir)?;
    let meta = fetch_json(str_at(entry, &["url"])?, &dir.join("version.json"))?;
    let java_major = meta["javaVersion"]["majorVersion"].as_u64().unwrap_or(0);

    let jar = dir.join("server.jar");
    download_verified(
        str_at(&meta, &["downloads", "server", "url"])?,
        str_at(&meta, &["downloads", "server", "sha1"])?,
        &jar,
    )?;

    let out = dir.join("gen");
    let _ = fs::remove_dir_all(&out);
    println!("running the data generator (needs Java {java_major})");
    let status = Command::new("java")
        .args(["-DbundlerMainClass=net.minecraft.data.Main", "-jar"])
        .arg(&jar)
        .args(["--reports", "--output"])
        .arg(&out)
        // The bundler unpacks `libraries/`, `versions/` and `logs/` into the working directory.
        .current_dir(&dir)
        .status()
        .map_err(|e| {
            format!("could not run `java` ({e}); run `mise install` to get Java {java_major}")
        })?;
    if !status.success() {
        return Err(
            format!("the data generator failed ({status}); it needs Java {java_major}").into(),
        );
    }

    let reports = out.join("reports");
    let read = |name: &str| -> Result<Value> {
        Ok(serde_json::from_slice(
            &fs::read(reports.join(name)).map_err(|e| format!("{name}: {e}"))?,
        )?)
    };
    let jar_version = unzip_json(&jar, "version.json")?;
    let input = codegen::Input {
        name: str_at(&jar_version, &["name"])?.to_owned(),
        protocol_version: int_at(&jar_version, "protocol_version")?,
        world_version: int_at(&jar_version, "world_version")?,
        blocks: read("blocks.json")?,
        packets: read("packets.json")?,
        registries: read("registries.json")?,
    };

    let target = root.join("crates/lodeframe-protocol/src/generated");
    for (file, source) in codegen::generate(&input)? {
        fs::write(target.join(file), source)?;
        println!("wrote {file}");
    }
    let fmt = Command::new("cargo")
        .args(["fmt", "-p", "lodeframe-protocol"])
        .status()?;
    if !fmt.success() {
        return Err("cargo fmt failed on the generated files".into());
    }
    println!(
        "generated data for {} (protocol {})",
        input.name, input.protocol_version
    );
    Ok(())
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Result<&'a str> {
    path.iter()
        .try_fold(v, |v, k| v.get(k))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string {}", path.join(".")).into())
}

fn int_at(v: &Value, key: &str) -> Result<i64> {
    v.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer {key}").into())
}

fn curl(url: &str, dest: &Path) -> Result<()> {
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "2", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("could not run `curl` ({e})"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("downloading {url} failed ({status})").into())
    }
}

fn fetch_json(url: &str, dest: &Path) -> Result<Value> {
    curl(url, dest)?;
    Ok(serde_json::from_slice(&fs::read(dest)?)?)
}

fn sha1_hex(path: &Path) -> Result<String> {
    Ok(Sha1::digest(fs::read(path)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Downloads `url` to `dest` unless a file with the expected sha1 is already there.
fn download_verified(url: &str, sha1: &str, dest: &Path) -> Result<()> {
    if dest.exists() && sha1_hex(dest)? == sha1 {
        println!("using cached {}", dest.display());
        return Ok(());
    }
    println!("downloading {url}");
    curl(url, dest)?;
    let actual = sha1_hex(dest)?;
    if actual != sha1 {
        let _ = fs::remove_file(dest);
        return Err(format!("sha1 mismatch for {url}: expected {sha1}, got {actual}").into());
    }
    Ok(())
}

fn unzip_json(jar: &PathBuf, name: &str) -> Result<Value> {
    let out = Command::new("unzip")
        .arg("-p")
        .arg(jar)
        .arg(name)
        .output()
        .map_err(|e| format!("could not run `unzip` ({e})"))?;
    if !out.status.success() {
        return Err(format!("{name} is not in {}", jar.display()).into());
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}
