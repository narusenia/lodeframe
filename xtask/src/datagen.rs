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

    let server_out = dir.join("gen-server");
    let _ = fs::remove_dir_all(&server_out);
    let status = Command::new("java")
        .args(["-DbundlerMainClass=net.minecraft.data.Main", "-jar"])
        .arg(&jar)
        .args(["--server", "--output"])
        .arg(&server_out)
        .current_dir(&dir)
        .status()?;
    if !status.success() {
        return Err(format!("the server data generator failed ({status})").into());
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
        datapack_registries: datapack_registries(&server_out, &read("datapack.json")?)?,
        tags: read_tags(&server_out, &read("registries.json")?)?,
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

/// Data-driven registries the client is sent during configuration.
///
/// This is `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` of the 26.3 server jar (read with
/// `javap -c -p`; the data generator reports do not include it). Re-read it when the
/// version changes: a registry missing here makes the client fail with "Missing element".
const SYNCED: &[&str] = &[
    "banner_pattern",
    "block_transformer",
    "cat_sound_variant",
    "cat_variant",
    "chat_type",
    "chicken_sound_variant",
    "chicken_variant",
    "cow_sound_variant",
    "cow_variant",
    "damage_type",
    "decorated_pot_pattern",
    "dialog",
    "dimension_type",
    "enchantment",
    "frog_variant",
    "instrument",
    "jukebox_song",
    "painting_variant",
    "pig_sound_variant",
    "pig_variant",
    "sulfur_cube_archetype",
    "test_environment",
    "test_instance",
    "timeline",
    "trim_material",
    "trim_pattern",
    "wolf_sound_variant",
    "wolf_variant",
    "world_clock",
    "worldgen/biome",
    "worldgen/block_state_provider",
    "zombie_nautilus_variant",
];

/// Entry names of the [`SYNCED`] registries, read from the `--server` output.
///
/// Each must be data-driven per `datapack.json`, so a renamed registry fails loudly.
fn datapack_registries(server_out: &Path, datapack: &Value) -> Result<Vec<(String, Vec<String>)>> {
    let mut registries = Vec::new();
    for name in SYNCED {
        let id = format!("minecraft:{name}");
        if datapack["registries"][&id]["elements"] != true {
            return Err(format!("{id} is not a data-driven registry in datapack.json").into());
        }
        let root = server_out.join("data/minecraft").join(name);
        let mut entries = Vec::new();
        collect_json(&root, &root, &mut entries)?;
        entries.sort();
        if entries.is_empty() {
            return Err(format!("{id} has no entries in the generated data").into());
        }
        registries.push((
            id,
            entries
                .into_iter()
                .map(|e| format!("minecraft:{e}"))
                .collect(),
        ));
    }
    Ok(registries)
}

/// Reads `data/minecraft/tags/<registry>/**.json` for every static registry and every
/// [`SYNCED`] one. Registries the client is never sent tags for (such as `villager_trade`)
/// are not read.
fn read_tags(
    server_out: &Path,
    registries: &Value,
) -> Result<std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<String>>>> {
    let static_names = registries
        .as_object()
        .ok_or("registries.json is not an object")?
        .keys()
        .filter_map(|k| k.strip_prefix("minecraft:"));
    let mut out = std::collections::BTreeMap::new();
    for registry in static_names.chain(SYNCED.iter().copied()) {
        let root = server_out.join("data/minecraft/tags").join(registry);
        if !root.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect_json(&root, &root, &mut files)?;
        let mut tags = std::collections::BTreeMap::new();
        for name in files {
            let json: Value =
                serde_json::from_slice(&fs::read(root.join(format!("{name}.json")))?)?;
            if json["replace"] == true {
                return Err(
                    format!("tag {registry}/{name} uses replace, which is not supported").into(),
                );
            }
            let values = json["values"]
                .as_array()
                .ok_or_else(|| format!("tag {registry}/{name} has no values"))?
                .iter()
                .map(|v| match v {
                    Value::String(s) => Ok(s.clone()),
                    // {"id": .., "required": ..}: optional entries are not used in vanilla's own tags
                    Value::Object(o) if o.get("required") != Some(&Value::Bool(false)) => o
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .ok_or_else(|| format!("tag {registry}/{name} has an entry without id")),
                    _ => Err(format!("tag {registry}/{name} has an unsupported entry")),
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            tags.insert(name, values);
        }
        out.insert(registry.to_owned(), tags);
    }
    Ok(out)
}

/// Pushes every `*.json` under `dir` as a path relative to `root`, without the extension.
fn collect_json(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_json(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "json") {
            let rel = path.strip_prefix(root)?.with_extension("");
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
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
