use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use rand::Rng;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SyftBoxConfig {
    data_dir: String,
    email: String,
    server_url: String,
    #[serde(default)]
    client_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(default)]
    dev_mode: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct EnvRegistry {
    environments: HashMap<String, EnvInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct EnvInfo {
    path: String,
    email: String,
    port: u16,
    name: String,
    #[serde(default)]
    server_url: String,
    #[serde(default)]
    dev_mode: bool,
    #[serde(default)]
    binary: Option<String>,
    #[serde(default)]
    binary_version: Option<String>,
    #[serde(default)]
    binary_hash: Option<String>,
    #[serde(default)]
    binary_os: Option<String>,
    #[serde(default)]
    binary_arch: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct GlobalConfig {
    #[serde(default)]
    default_binary: Option<String>, // path or version
}

#[derive(Parser)]
#[command(name = "sbenv")]
#[command(author, version, about = "SyftBox Env - virtualenv for SyftBox", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new SyftBox environment in the current directory
    Init {
        /// Email address for the datasite
        #[arg(short, long)]
        email: Option<String>,
        /// SyftBox server URL (overrides default)
        #[arg(short, long)]
        server_url: Option<String>,
        /// Enable development mode defaults
        #[arg(long, default_value_t = false)]
        dev: bool,
        /// Specify syftbox binary (path) or version (e.g. 0.8.5)
        #[arg(long)]
        binary: Option<String>,
        /// Run in quiet mode, automatically accepting defaults
        #[arg(short, long, default_value_t = false)]
        quiet: bool,
    },
    /// Edit current environment settings (server URL, dev mode)
    Edit {
        /// SyftBox server URL
        #[arg(long)]
        server_url: Option<String>,
        /// Toggle development mode on/off
        #[arg(long)]
        dev: Option<bool>,
        /// Change syftbox binary (path) or version
        #[arg(long)]
        binary: Option<String>,
    },
    /// Display information about the current environment
    Info,
    /// Activate the SyftBox environment (outputs shell commands to eval)
    Activate {
        /// Write activation script to a file instead of stdout
        #[arg(short, long)]
        write_to: Option<PathBuf>,
        /// Suppress instructional comments (for shell function use)
        #[arg(short, long)]
        quiet: bool,
    },
    /// Deactivate the SyftBox environment (outputs shell commands to eval)
    Deactivate {
        /// Suppress instructional comments (for shell function use)
        #[arg(short, long)]
        quiet: bool,
    },
    /// Remove a SyftBox environment
    Remove {
        /// Path to the environment to remove (defaults to current directory)
        path: Option<PathBuf>,
    },
    /// Install shell functions for easier activation/deactivation
    InstallShell {
        /// Show manual installation instructions instead of auto-installing
        #[arg(short, long)]
        manual: bool,
    },
    /// Start the SyftBox daemon in the background
    Start {
        /// Force start even if another instance is running
        #[arg(short, long)]
        force: bool,
        /// Skip login check
        #[arg(long)]
        skip_login_check: bool,
        /// Run syftbox in daemon mode (background). By default off; process is still backgrounded.
        #[arg(long, default_value_t = false)]
        daemon: bool,
    },
    /// Stop the running SyftBox daemon
    Stop,
    /// Show status of the SyftBox daemon
    Status,
    /// Restart the SyftBox daemon
    Restart,
    /// Show daemon logs
    Logs {
        /// Number of lines to show (default: follow mode)
        #[arg(short = 'n', long)]
        lines: Option<usize>,
        /// Follow log output
        #[arg(short, long, default_value = "true")]
        follow: bool,
    },
    /// Login to SyftBox
    Login,
    /// List all SyftBox environments
    List,
    /// Update sbenv to the latest version
    Update {
        /// Force update without confirmation
        #[arg(short, long)]
        force: bool,
    },
}

fn get_registry_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".sbenv").join("envs.json")
}

fn get_global_config_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".sbenv").join("config.json")
}

fn load_registry() -> Result<EnvRegistry> {
    let registry_path = get_registry_path();
    if !registry_path.exists() {
        return Ok(EnvRegistry {
            environments: HashMap::new(),
        });
    }
    let content = fs::read_to_string(&registry_path)?;
    let registry: EnvRegistry = serde_json::from_str(&content)?;
    Ok(registry)
}

fn save_registry(registry: &EnvRegistry) -> Result<()> {
    let registry_path = get_registry_path();
    if let Some(parent) = registry_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&registry)?;
    fs::write(&registry_path, content)?;
    Ok(())
}

fn load_global_config() -> GlobalConfig {
    let path = get_global_config_path();
    if !path.exists() {
        return GlobalConfig::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<GlobalConfig>(&s).ok())
        .unwrap_or_default()
}

fn save_global_config(cfg: &GlobalConfig) -> Result<()> {
    let path = get_global_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(cfg)?;
    fs::write(path, s)?;
    Ok(())
}

fn get_used_ports() -> Result<Vec<u16>> {
    let registry = load_registry()?;
    Ok(registry.environments.values().map(|e| e.port).collect())
}

fn find_available_port() -> Result<u16> {
    let used_ports = get_used_ports()?;
    let mut rng = rand::thread_rng();

    for _ in 0..100 {
        let port = rng.gen_range(7939..=7999);
        if !used_ports.contains(&port) {
            return Ok(port);
        }
    }

    Err(anyhow::anyhow!("No available ports in range 7939-7999"))
}

fn generate_env_key(path: &Path, email: &str) -> String {
    // Create a unique key using email and absolute path
    // This ensures multiple environments with same directory name don't conflict
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_str = abs_path.to_string_lossy();
    format!("{}@{}", email, path_str)
}

fn register_environment(path: &Path, config: &SyftBoxConfig) -> Result<()> {
    let mut registry = load_registry()?;

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let key = generate_env_key(path, &config.email);

    let port = config
        .client_url
        .as_deref()
        .and_then(|u| u.rsplit(':').next())
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(0);

    // Preserve existing binary info if present
    let existing = registry.environments.get(&key).cloned();
    let env_info = EnvInfo {
        path: path.to_string_lossy().to_string(),
        email: config.email.clone(),
        port,
        name: name.clone(),
        server_url: config.server_url.clone(),
        dev_mode: config.dev_mode,
        binary: existing.as_ref().and_then(|e| e.binary.clone()),
        binary_version: existing.as_ref().and_then(|e| e.binary_version.clone()),
        binary_hash: existing.as_ref().and_then(|e| e.binary_hash.clone()),
        binary_os: existing.as_ref().and_then(|e| e.binary_os.clone()),
        binary_arch: existing.as_ref().and_then(|e| e.binary_arch.clone()),
    };

    registry.environments.insert(key, env_info);
    save_registry(&registry)?;
    Ok(())
}

fn unregister_environment(path: &Path) -> Result<()> {
    let mut registry = load_registry()?;
    let path_str = path.to_string_lossy().to_string();

    registry
        .environments
        .retain(|_, info| info.path != path_str);
    save_registry(&registry)?;
    Ok(())
}

fn ensure_marker_exists(config_path: &Path, config: &SyftBoxConfig) -> Result<()> {
    // Ensure a .sbenv marker exists in the environment root
    let env_dir = config_path
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Invalid config path layout"))?;
    let marker = env_dir.join(".sbenv");
    if marker.exists() {
        return Ok(());
    }

    // Determine port: prefer config.client_url, fallback to registry by path, else 0
    let port_from_config = config
        .client_url
        .as_deref()
        .and_then(|u| u.rsplit(':').next())
        .and_then(|p| p.parse::<u16>().ok());
    let port = if let Some(p) = port_from_config {
        p
    } else {
        let registry = load_registry().unwrap_or(EnvRegistry {
            environments: HashMap::new(),
        });
        let env_key = generate_env_key(env_dir, &config.email);
        registry
            .environments
            .get(&env_key)
            .map(|info| info.port)
            .unwrap_or(0)
    };

    // Get binary info from registry if available
    let registry = load_registry().unwrap_or(EnvRegistry {
        environments: HashMap::new(),
    });
    let env_key = generate_env_key(env_dir, &config.email);
    let binary_info = registry.environments.get(&env_key);

    let mut obj = serde_json::json!({
        "email": config.email,
        "port": port,
        "server_url": config.server_url,
    });

    // Add binary fields if available
    if let Some(info) = binary_info {
        if let Some(b) = &info.binary {
            obj["binary"] = serde_json::json!(b);
        }
        if let Some(v) = &info.binary_version {
            obj["binary_version"] = serde_json::json!(v);
        }
        if let Some(h) = &info.binary_hash {
            obj["binary_hash"] = serde_json::json!(h);
        }
        if let Some(o) = &info.binary_os {
            obj["binary_os"] = serde_json::json!(o);
        }
        if let Some(a) = &info.binary_arch {
            obj["binary_arch"] = serde_json::json!(a);
        }
    }
    let content = serde_json::to_string_pretty(&obj)? + "\n";
    fs::write(&marker, content)?;
    Ok(())
}

fn get_binaries_dir() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".sbenv").join("binaries")
}

fn parse_syftbox_version_output(output: &str) -> Option<String> {
    // Expected: syftbox version 0.8.5 (...)
    let lower = output.trim();
    let parts: Vec<&str> = lower.split_whitespace().collect();
    let idx = parts.iter().position(|p| *p == "version")?;
    parts.get(idx + 1).map(|s| s.to_string())
}

#[derive(Debug, Clone, Default)]
struct SyftboxDetails {
    version: Option<String>,
    hash: Option<String>,
    go_version: Option<String>,
    os: Option<String>,
    arch: Option<String>,
    build_time: Option<String>,
}

fn parse_syftbox_details(output: &str) -> SyftboxDetails {
    // syftbox version 0.8.5 (26645a3; go1.24.3; darwin/arm64; 2025-09-16T04:17:56Z)
    let mut det = SyftboxDetails {
        version: parse_syftbox_version_output(output),
        ..Default::default()
    };
    if let Some(start) = output.find('(') {
        if let Some(end) = output[start + 1..].find(')') {
            let inner = &output[start + 1..start + 1 + end];
            let parts: Vec<&str> = inner.split(';').map(|s| s.trim()).collect();
            if let Some(hash) = parts.first() {
                if !hash.is_empty() {
                    det.hash = Some((*hash).to_string());
                }
            }
            if let Some(go) = parts.get(1) {
                if !go.is_empty() {
                    det.go_version = Some((*go).to_string());
                }
            }
            if let Some(target) = parts.get(2) {
                if let Some((os, arch)) = target.split_once('/') {
                    det.os = Some(os.to_string());
                    det.arch = Some(arch.to_string());
                }
            }
            if let Some(bt) = parts.get(3) {
                if !bt.is_empty() {
                    det.build_time = Some((*bt).to_string());
                }
            }
        }
    }
    det
}

fn detect_binary_details(bin: &Path) -> SyftboxDetails {
    let out = Command::new(bin).arg("--version").output();
    if let Ok(out) = out {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            return parse_syftbox_details(&s);
        }
    }
    SyftboxDetails::default()
}

fn which_syftbox() -> Option<PathBuf> {
    let out = Command::new("which").arg("syftbox").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() {
        None
    } else {
        Some(PathBuf::from(p))
    }
}

fn get_cached_syftbox_versions() -> Vec<String> {
    let bin_dir = get_binaries_dir();
    if !bin_dir.exists() {
        return Vec::new();
    }

    let mut versions = Vec::new();
    if let Ok(entries) = fs::read_dir(&bin_dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        // Check if there's actually a syftbox binary in this directory
                        let bin_path = entry.path().join("syftbox");
                        if bin_path.exists() {
                            versions.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Sort versions in reverse order (newest first)
    versions.sort_by(|a, b| match (Version::parse(a), Version::parse(b)) {
        (Ok(va), Ok(vb)) => vb.cmp(&va),
        _ => b.cmp(a),
    });

    versions
}

fn fetch_latest_syftbox_version() -> Result<String> {
    let url = "https://api.github.com/repos/OpenMined/syftbox/releases/latest";
    let out = Command::new("curl")
        .args(["-sL", "-H", "User-Agent: sbenv", url])
        .output()?;

    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch latest release info from GitHub"
        ));
    }

    let body = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&body)?;

    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("Could not find tag_name in release"))?;

    // Remove 'v' prefix if present
    let version = tag.strip_prefix('v').unwrap_or(tag);

    Ok(version.to_string())
}

fn prompt_for_syftbox_install() -> Result<Option<String>> {
    println!("{}", "⚠️  SyftBox is not installed in your PATH.".yellow());
    println!();

    // Check for cached versions
    let cached_versions = get_cached_syftbox_versions();

    // Try to fetch the latest version
    let latest_version = fetch_latest_syftbox_version().ok();

    if cached_versions.is_empty() && latest_version.is_none() {
        println!(
            "{}",
            "❌ Could not fetch available versions and no cached versions found.".red()
        );
        println!("Please install syftbox manually or ensure internet connectivity.");
        return Ok(None);
    }

    let mut options = Vec::new();

    // Add latest version if available
    if let Some(ref latest) = latest_version {
        options.push(format!("Download latest version ({})", latest));
    }

    // Add cached versions
    for version in &cached_versions {
        options.push(format!("Use cached version {}", version));
    }

    options.push("Skip (I'll install it manually)".to_string());

    println!("What would you like to do?");
    println!();

    for (i, option) in options.iter().enumerate() {
        println!("  {}. {}", i + 1, option);
    }
    println!();

    let selection = Input::<usize>::with_theme(&ColorfulTheme::default())
        .with_prompt("Select an option")
        .validate_with(|n: &usize| {
            if *n > 0 && *n <= options.len() {
                Ok(())
            } else {
                Err(format!(
                    "Please enter a number between 1 and {}",
                    options.len()
                ))
            }
        })
        .interact()?;

    if selection == options.len() {
        // User chose to skip
        return Ok(None);
    }

    if let Some(ref latest) = latest_version {
        if selection == 1 {
            // Download latest version
            return Ok(Some(latest.clone()));
        }
    }

    // User selected a cached version
    let cached_idx = if latest_version.is_some() {
        selection - 2 // Adjust index if latest version was in the list
    } else {
        selection - 1
    };

    if cached_idx < cached_versions.len() {
        Ok(Some(cached_versions[cached_idx].clone()))
    } else {
        Ok(None)
    }
}

fn current_os_arch() -> (String, String) {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    (os.to_string(), arch.to_string())
}

#[allow(clippy::needless_borrows_for_generic_args)]
fn ensure_syftbox_version(version: &str) -> Result<PathBuf> {
    let bin_dir = get_binaries_dir().join(version);
    let bin_path = bin_dir.join("syftbox");
    if bin_path.exists() {
        println!("   Using cached syftbox version {}", version.cyan());
        return Ok(bin_path);
    }

    println!("   Downloading syftbox version {}...", version.cyan());
    fs::create_dir_all(&bin_dir)?;
    let (os, arch) = current_os_arch();
    let base = format!(
        "https://github.com/OpenMined/syftbox/releases/download/v{}/",
        version
    );
    let candidates = vec![
        format!("syftbox_{}_{}_{}.tar.gz", version, os, arch),
        format!("syftbox-{}-{}-{}.tar.gz", version, os, arch),
        format!("syftbox_{}_{}_{}.zip", version, os, arch),
        format!("syftbox-{}-{}-{}.zip", version, os, arch),
        format!("syftbox_{}_{}_{}", version, os, arch),
        format!("syftbox-{}-{}-{}", version, os, arch),
    ];

    let tmp_dir = bin_dir.join("_tmp");
    let _ = fs::remove_dir_all(&tmp_dir);
    fs::create_dir_all(&tmp_dir)?;

    let mut last_err: Option<anyhow::Error> = None;

    // Try GitHub API to find the correct asset for this OS/arch
    if let Some((asset_url, asset_name)) = github_release_asset_for(version) {
        let tmp_file = tmp_dir.join("download_asset");
        let status = Command::new("curl")
            .args(["-fL", "-o", tmp_file.to_str().unwrap(), &asset_url])
            .status();
        if let Ok(st) = status {
            if st.success() {
                if let Err(e) =
                    install_syftbox_from_download(&tmp_file, &asset_name, &tmp_dir, &bin_path)
                {
                    last_err = Some(e);
                } else {
                    let _ = fs::remove_dir_all(&tmp_dir);
                    return Ok(bin_path);
                }
            }
        }
    }
    for name in candidates {
        let url = format!("{}{}", base, name);
        let tmp_file = tmp_dir.join("download.bin");
        let status = Command::new("curl")
            .args(["-fL", "-o", tmp_file.to_str().unwrap(), &url])
            .status();
        if let Ok(st) = status {
            if st.success() {
                // Try to detect archive by extension
                let lower = name.to_lowercase();
                if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
                    let st2 = Command::new("tar")
                        .args([
                            "-xzf",
                            tmp_file.to_str().unwrap(),
                            "-C",
                            tmp_dir.to_str().unwrap(),
                        ])
                        .status();
                    if st2.as_ref().map(|s| s.success()).unwrap_or(false) {
                        // find a file named syftbox in tmp_dir tree
                        if let Some(found) = find_in_dir(&tmp_dir, "syftbox") {
                            fs::rename(&found, &bin_path)?;
                            let _ = fs::remove_dir_all(&tmp_dir);
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                let mut perm = fs::metadata(&bin_path)?.permissions();
                                perm.set_mode(0o755);
                                fs::set_permissions(&bin_path, perm)?;
                            }
                            return Ok(bin_path);
                        }
                    }
                } else if lower.ends_with(".zip") {
                    // Try unzip
                    let st2 = Command::new("unzip")
                        .args([
                            "-o",
                            tmp_file.to_str().unwrap(),
                            "-d",
                            tmp_dir.to_str().unwrap(),
                        ])
                        .status();
                    if st2.as_ref().map(|s| s.success()).unwrap_or(false) {
                        if let Some(found) = find_in_dir(&tmp_dir, "syftbox") {
                            fs::rename(&found, &bin_path)?;
                            let _ = fs::remove_dir_all(&tmp_dir);
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                let mut perm = fs::metadata(&bin_path)?.permissions();
                                perm.set_mode(0o755);
                                fs::set_permissions(&bin_path, perm)?;
                            }
                            return Ok(bin_path);
                        }
                    }
                } else {
                    // Assume it's the binary itself
                    fs::rename(&tmp_file, &bin_path)?;
                    let _ = fs::remove_dir_all(&tmp_dir);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mut perm = fs::metadata(&bin_path)?.permissions();
                        perm.set_mode(0o755);
                        fs::set_permissions(&bin_path, perm)?;
                    }
                    return Ok(bin_path);
                }
            }
        } else if let Err(e) = status {
            last_err = Some(anyhow::anyhow!("curl failed: {}", e));
        }
    }
    let _ = fs::remove_dir_all(&tmp_dir);
    if let Some(e) = last_err {
        Err(e)
    } else {
        let (os2, arch2) = current_os_arch();
        Err(anyhow::anyhow!(
            "Failed to download syftbox {} for {}-{}",
            version,
            os2,
            arch2
        ))
    }
}

fn github_release_asset_for(version: &str) -> Option<(String, String)> {
    // Use GitHub API to get assets for the tag and choose the best match
    let url = format!(
        "https://api.github.com/repos/OpenMined/syftbox/releases/tags/v{}",
        version
    );
    let out = Command::new("curl")
        .args(["-sL", "-H", "User-Agent: sbenv", &url])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let assets = v.get("assets")?.as_array()?;
    let (os, arch) = current_os_arch();
    let os_tokens = match os.as_str() {
        "darwin" => vec!["darwin", "macos", "osx", "apple-darwin"],
        "linux" => vec!["linux", "gnu", "musl", "linux-gnu"],
        other => vec![other],
    };
    let arch_tokens = match arch.as_str() {
        "arm64" => vec!["arm64", "aarch64"],
        "x86_64" => vec!["x86_64", "amd64"],
        other => vec![other],
    };
    let mut best: Option<(String, String, i32)> = None; // (url, name, score)
    for a in assets {
        let name = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let url = a
            .get("browser_download_url")
            .and_then(|u| u.as_str())
            .unwrap_or("");
        let lname = name.to_lowercase();
        if !lname.contains("syftbox") {
            continue;
        }
        if !os_tokens.iter().any(|t| lname.contains(t)) {
            continue;
        }
        if !arch_tokens.iter().any(|t| lname.contains(t)) {
            continue;
        }
        let score = if lname.ends_with(".tar.gz") || lname.ends_with(".tgz") {
            3
        } else if lname.ends_with(".zip") {
            2
        } else {
            1
        };
        match &best {
            None => best = Some((url.to_string(), name.to_string(), score)),
            Some((_, _, bs)) if score > *bs => {
                best = Some((url.to_string(), name.to_string(), score))
            }
            _ => {}
        }
        if score == 3 {
            // Good enough
            break;
        }
    }
    best.map(|(u, n, _)| (u, n))
}

#[allow(clippy::needless_borrows_for_generic_args)]
fn install_syftbox_from_download(
    tmp_file: &Path,
    asset_name: &str,
    tmp_dir: &Path,
    bin_path: &Path,
) -> Result<()> {
    let lname = asset_name.to_lowercase();
    if lname.ends_with(".tar.gz") || lname.ends_with(".tgz") {
        let st2 = Command::new("tar")
            .args([
                "-xzf",
                tmp_file.to_str().unwrap(),
                "-C",
                tmp_dir.to_str().unwrap(),
            ])
            .status()?;
        if !st2.success() {
            return Err(anyhow::anyhow!("Failed to extract tar.gz"));
        }
        let found = find_in_dir(tmp_dir, "syftbox")
            .ok_or_else(|| anyhow::anyhow!("syftbox binary not found in archive"))?;
        fs::rename(&found, &bin_path)?;
    } else if lname.ends_with(".zip") {
        let st2 = Command::new("unzip")
            .args([
                "-o",
                tmp_file.to_str().unwrap(),
                "-d",
                tmp_dir.to_str().unwrap(),
            ])
            .status()?;
        if !st2.success() {
            return Err(anyhow::anyhow!("Failed to unzip asset"));
        }
        let found = find_in_dir(tmp_dir, "syftbox")
            .ok_or_else(|| anyhow::anyhow!("syftbox binary not found in zip"))?;
        fs::rename(&found, &bin_path)?;
    } else {
        // Assume it's the binary itself
        fs::rename(&tmp_file, &bin_path)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&bin_path)?.permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&bin_path, perm)?;
    }
    Ok(())
}

fn find_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(read) = fs::read_dir(&d) {
            for e in read.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p.clone());
                }
                if p.file_name().and_then(|n| n.to_str()) == Some(name) {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn resolve_or_install_syftbox(spec: &str) -> Result<(PathBuf, Option<String>)> {
    // If spec parses as semver => version
    if Version::parse(spec).is_ok() {
        let bin = ensure_syftbox_version(spec)?;
        let ver = detect_binary_version(&bin);
        return Ok((bin, ver));
    }
    // Otherwise treat as path
    let p = PathBuf::from(spec);
    let path = if p.is_absolute() || p.exists() {
        p
    } else {
        // Fallback: try PATH command name
        which_syftbox().unwrap_or_else(|| PathBuf::from("syftbox"))
    };
    let ver = detect_binary_version(&path);
    Ok((path, ver))
}

fn detect_binary_version(bin: &Path) -> Option<String> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_syftbox_version_output(&String::from_utf8_lossy(&out.stdout))
}

fn resolve_binary_for_env(config_path: &Path) -> Result<(PathBuf, Option<String>)> {
    // Load config to get email for key generation
    let config = load_config(config_path)?;

    // Prefer env-specific registry entry
    let registry = load_registry()?;
    let env_dir = config_path.parent().unwrap().parent().unwrap();
    let env_key = generate_env_key(env_dir, &config.email);
    let entry = registry.environments.get(&env_key);
    if let Some(info) = entry {
        if let Some(ver) = &info.binary_version {
            if Version::parse(ver).is_ok() {
                let bin = ensure_syftbox_version(ver)?;
                let v = detect_binary_version(&bin).or_else(|| Some(ver.clone()));
                return Ok((bin, v));
            }
        }
        if let Some(b) = &info.binary {
            let p = PathBuf::from(b);
            return Ok((p.clone(), detect_binary_version(&p)));
        }
    }
    // Fallback to global default
    let gc = load_global_config();
    if let Some(spec) = gc.default_binary {
        return resolve_or_install_syftbox(&spec);
    }
    // Fallback to PATH
    if let Some(p) = which_syftbox() {
        return Ok((p.clone(), detect_binary_version(&p)));
    }
    // Final fallback: plain name (might fail at runtime)
    Ok((PathBuf::from("syftbox"), None))
}

fn ensure_env_has_binary(env_dir: &Path, email: &str) -> Result<()> {
    let env_key = generate_env_key(env_dir, email);
    let mut registry = load_registry()?;
    if let Some(info) = registry.environments.get_mut(&env_key) {
        if info.binary.is_none() && info.binary_version.is_none() {
            let gc = load_global_config();
            if let Some(spec) = gc.default_binary {
                let (p, v) = resolve_or_install_syftbox(&spec)?;
                info.binary = Some(p.to_string_lossy().to_string());
                info.binary_version = v.clone();
                let d = detect_binary_details(&p);
                info.binary_hash = d.hash;
                info.binary_os = d.os;
                info.binary_arch = d.arch;
                save_registry(&registry)?;
            } else if let Some(p) = which_syftbox() {
                info.binary = Some(p.to_string_lossy().to_string());
                let d = detect_binary_details(&p);
                info.binary_version = d.version;
                info.binary_hash = d.hash;
                info.binary_os = d.os;
                info.binary_arch = d.arch;
                save_registry(&registry)?;
            }
        }
    }
    Ok(())
}
fn find_syftbox_config(start_dir: &Path) -> Option<PathBuf> {
    let mut current = start_dir.to_path_buf();
    loop {
        let config_path = current.join(".syftbox").join("config.json");
        if config_path.exists() {
            return Some(config_path);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn load_config(config_path: &Path) -> Result<SyftBoxConfig> {
    let content = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config from {:?}", config_path))?;
    let config: SyftBoxConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse config from {:?}", config_path))?;
    Ok(config)
}

fn init_environment_with_binary(
    email: Option<String>,
    server_url: Option<String>,
    dev: bool,
    binary: Option<String>,
    quiet: bool,
) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let syftbox_dir = current_dir.join(".syftbox");

    if syftbox_dir.exists() {
        println!(
            "{}",
            "A SyftBox environment already exists in this directory!".red()
        );
        return Ok(());
    }

    let email = if let Some(email) = email {
        email
    } else {
        if quiet {
            return Err(anyhow::anyhow!(
                "Email address is required when using --quiet flag. Use -e <email> to provide it."
            ));
        }
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Email address")
            .interact_text()
            .context("Failed to read email input")?
    };

    let port = find_available_port().context("Failed to find available port")?;
    let client_url = format!("http://127.0.0.1:{}", port);
    let resolved_server_url = match server_url {
        Some(url) => url,
        None => {
            if dev {
                "http://localhost:8080".to_string()
            } else {
                "https://syftbox.net".to_string()
            }
        }
    };

    let config = SyftBoxConfig {
        data_dir: current_dir.to_string_lossy().to_string(),
        email: email.clone(),
        server_url: resolved_server_url.clone(),
        client_url: Some(client_url.clone()),
        client_token: None,
        refresh_token: None,
        dev_mode: dev,
    };

    fs::create_dir_all(&syftbox_dir).context("Failed to create .syftbox directory")?;

    let config_path = syftbox_dir.join("config.json");
    let config_json =
        serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    fs::write(&config_path, config_json).context("Failed to write config file")?;

    register_environment(&current_dir, &config)?;

    // Check if syftbox is available, prompt for install if not
    let binary_to_use = if binary.is_some() {
        binary
    } else {
        // Check if syftbox is in PATH
        let syftbox_in_path = which_syftbox().is_some();

        if !syftbox_in_path {
            if quiet {
                // In quiet mode, automatically try to get the latest version
                println!("📦 SyftBox not found in PATH. Fetching latest version...");
                match fetch_latest_syftbox_version() {
                    Ok(version) => Some(version),
                    Err(e) => {
                        // If we can't fetch latest, try to use a cached version
                        let cached = get_cached_syftbox_versions();
                        if !cached.is_empty() {
                            println!("   Could not fetch latest version: {}", e);
                            println!("   Using cached version: {}", cached[0]);
                            Some(cached[0].clone())
                        } else {
                            println!("⚠️  Could not fetch latest version and no cached versions available");
                            println!("   Error: {}", e);
                            None
                        }
                    }
                }
            } else {
                // Interactive mode - prompt user
                // This will show cached versions and option to download latest
                if let Ok(Some(version)) = prompt_for_syftbox_install() {
                    Some(version)
                } else {
                    None
                }
            }
        } else {
            None
        }
    };

    // Resolve and persist binary preference
    if let Some(bin_spec) = binary_to_use {
        println!("📦 Setting up SyftBox binary...");
        let (bin_path, bin_ver) = resolve_or_install_syftbox(&bin_spec)?;
        println!("✅ SyftBox binary configured successfully!");
        // Update registry entry
        let mut registry = load_registry()?;
        let env_key = generate_env_key(&current_dir, &email);
        if let Some(info) = registry.environments.get_mut(&env_key) {
            info.binary = Some(bin_path.to_string_lossy().to_string());
            info.binary_version = bin_ver;
            let d = detect_binary_details(&bin_path);
            info.binary_hash = d.hash;
            info.binary_os = d.os;
            info.binary_arch = d.arch;
        }
        save_registry(&registry)?;

        // Save as global default for future envs
        let mut gc = load_global_config();
        gc.default_binary = Some(bin_spec);
        let _ = save_global_config(&gc);
    } else {
        // If no spec, ensure global default exists (noop if not set)
        let _ = ensure_env_has_binary(&current_dir, &email);
    }

    // Write a marker file so other tools can detect the environment
    // This will include binary info if it was set in the registry
    let _ = ensure_marker_exists(&config_path, &config);

    println!("{}", "✅ SyftBox environment initialized!".green().bold());
    println!();
    println!("📧 Email: {}", email.cyan());
    println!("🌐 Server: {}", resolved_server_url.cyan());
    println!("📁 Data dir: {}", current_dir.display().to_string().cyan());
    println!("🔌 Client port: {}", port.to_string().cyan());
    // Show resolved binary information
    if let Ok((bin_path, bin_ver)) = resolve_binary_for_env(&config_path) {
        println!("🛠 Binary: {}", bin_path.display().to_string().cyan());
        if let Some(v) = bin_ver {
            println!("🔢 Version: {}", v.cyan());
        }
    }
    println!();
    println!("Run {} to see this info again", "sbenv info".yellow());
    println!(
        "Run {} to activate this environment",
        "sbenv activate".yellow()
    );

    Ok(())
}

fn show_info() -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir).ok_or_else(|| {
        anyhow::anyhow!("No SyftBox environment found in current directory or parents")
    })?;

    let config = load_config(&config_path)?;

    // Register the environment if not already registered
    let env_dir = config_path.parent().unwrap().parent().unwrap();
    let _ = register_environment(env_dir, &config);

    let _port = config
        .client_url
        .as_deref()
        .and_then(|u| u.rsplit(':').next())
        .unwrap_or("unknown");

    println!("{}", "📦 SyftBox Environment Info".green().bold());
    println!();
    println!("{}", "── Local Environment ──".dimmed());
    println!("📧 Email: {}", config.email.cyan());
    println!("🌐 Server URL: {}", config.server_url.cyan());
    println!("📁 Data dir: {}", config.data_dir.cyan());
    println!(
        "🔌 Client URL: {}",
        config.client_url.as_deref().unwrap_or("not set").cyan()
    );
    println!(
        "⚙️  Dev mode: {}",
        if config.dev_mode {
            "enabled".green()
        } else {
            "disabled".dimmed()
        }
    );
    println!(
        "📄 Config path: {}",
        config_path.display().to_string().cyan()
    );

    // Show binary details resolved for this environment
    if let Ok((bin_path, bin_ver)) = resolve_binary_for_env(&config_path) {
        println!("🛠 Binary: {}", bin_path.display().to_string().cyan());
        if let Some(v) = bin_ver {
            println!("🔢 Version: {}", v.cyan());
        }
        let d = detect_binary_details(&bin_path);
        if let Some(h) = d.hash {
            println!("    Hash: {}", h.cyan());
        }
        if let (Some(os), Some(arch)) = (d.os, d.arch) {
            println!("    Target: {}/{}", os.cyan(), arch.cyan());
        }
    }

    // Show raw config.json content
    println!();
    println!("{}", "── Config File Content ──".dimmed());
    if let Ok(config_content) = fs::read_to_string(&config_path) {
        // Parse and pretty print the JSON
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&config_content) {
            if let Ok(pretty_json) = serde_json::to_string_pretty(&json_value) {
                for line in pretty_json.lines() {
                    println!("  {}", line.dimmed());
                }
            }
        }
    }

    // Show global sbenv registry info
    println!();
    println!("{}", "── Global sbenv Registry ──".dimmed());
    let registry = load_registry().unwrap_or(EnvRegistry {
        environments: HashMap::new(),
    });

    let env_dir_str = env_dir.to_string_lossy().to_string();
    let env_name = env_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let env_key = generate_env_key(env_dir, &config.email);

    if let Some(env_info) = registry.environments.get(&env_key) {
        println!("📝 Registered as: {}", env_name.cyan());
        println!("🏠 Location: {}", env_info.path.cyan());
        println!("🔗 Server: {}", env_info.server_url.cyan());
        println!("🔌 Port: {}", env_info.port.to_string().cyan());
        println!(
            "⚙️  Dev mode: {}",
            if env_info.dev_mode {
                "enabled".green()
            } else {
                "disabled".dimmed()
            }
        );
        if let Some(b) = &env_info.binary {
            println!("🛠  Binary: {}", b.cyan());
        }
        if let Some(v) = &env_info.binary_version {
            println!("    Version: {}", v.cyan());
        }
        if let Some(h) = &env_info.binary_hash {
            println!("    Hash: {}", h.cyan());
        }
        if env_info.binary_os.is_some() || env_info.binary_arch.is_some() {
            println!(
                "    Target: {}/{}",
                env_info.binary_os.as_deref().unwrap_or("?"),
                env_info.binary_arch.as_deref().unwrap_or("?")
            );
        }
    } else {
        println!("⚠️  Not registered in global sbenv");
        println!("   Run {} to register", "sbenv init".yellow());
    }

    // Show all registered environments
    if registry.environments.len() > 1 {
        println!();
        println!("{}", "── Other Environments ──".dimmed());
        for (name, info) in registry.environments.iter() {
            if info.path != env_dir_str {
                println!("  • {} ({})", name.cyan(), info.path.dimmed());
            }
        }
    }

    Ok(())
}

fn activate_environment(quiet: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir).ok_or_else(|| {
        anyhow::anyhow!("No SyftBox environment found in current directory or parents")
    })?;

    let config = load_config(&config_path)?;

    // Ensure .sbenv marker exists for this environment
    let _ = ensure_marker_exists(&config_path, &config);

    if !quiet {
        println!("# Run this command to activate the environment:");
        println!("# eval \"$(sbenv activate)\"");
        println!();
    }

    println!("export SYFTBOX_EMAIL=\"{}\"", config.email);
    println!("export SYFTBOX_DATA_DIR=\"{}\"", config.data_dir);
    println!("export SYFTBOX_SERVER_URL=\"{}\"", config.server_url);
    println!("export SYFTBOX_CONFIG_PATH=\"{}\"", config_path.display());
    if let Some(url) = &config.client_url {
        println!("export SYFTBOX_CLIENT_URL=\"{}\"", url);
    }
    // Resolve syftbox binary + version for this env (fallback to 'syftbox')
    let (bin_path, bin_ver) =
        resolve_binary_for_env(&config_path).unwrap_or((PathBuf::from("syftbox"), None));
    println!("export SYFTBOX_BINARY=\"{}\"", bin_path.display());
    if let Some(v) = bin_ver {
        println!("export SYFTBOX_VERSION=\"{}\"", v);
    }
    let d = detect_binary_details(&bin_path);
    if let Some(h) = d.hash {
        println!("export SYFTBOX_BUILD_HASH=\"{}\"", h);
    }
    if let (Some(os), Some(arch)) = (d.os, d.arch) {
        println!("export SYFTBOX_BUILD_TARGET=\"{}/{}\"", os, arch);
    }
    println!("export SYFTBOX_ENV_ACTIVE=\"1\"");

    // Use email as the environment name for better identification
    let env_name = config.email.clone();

    println!("export SYFTBOX_ENV_NAME=\"{}\"", env_name);

    // Use VIRTUAL_ENV for compatibility with Powerlevel10k and other prompt tools
    println!("export SYFTBOX_OLD_VIRTUAL_ENV=\"$VIRTUAL_ENV\"");
    println!("export VIRTUAL_ENV=\"{}\"", config.data_dir);

    // Customize Powerlevel10k virtualenv display
    println!("if [ -n \"$ZSH_VERSION\" ]; then");
    println!("    # For Powerlevel10k - override the virtualenv display format");
    println!("    if typeset -f _p9k_prompt_virtualenv_init >/dev/null 2>&1; then");
    println!("        # Save old format settings");
    println!(
        "        export SYFTBOX_OLD_P9K_CONTENT=\"${{POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION}}\""
    );
    println!(
        "        export SYFTBOX_OLD_P9K_VISUAL=\"${{POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION}}\""
    );
    println!("        # Override to show box icon and email without 'Py'");
    println!(
        "        export POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION='📦 {}'",
        config.email
    );
    println!("        export POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION=''");
    println!("        export POWERLEVEL9K_VIRTUALENV_SHOW_PYTHON_VERSION=false");
    println!("        export POWERLEVEL9K_VIRTUALENV_SHOW_WITH_PYENV=false");
    println!("    fi");
    println!("    # For non-Powerlevel10k ZSH");
    println!("    if [ -z \"$POWERLEVEL9K_LEFT_PROMPT_ELEMENTS\" ]; then");
    println!("        export SYFTBOX_OLD_PS1=\"$PS1\"");
    println!("        export PS1=\"📦 ($SYFTBOX_ENV_NAME) $PS1\"");
    println!("    fi");
    println!("else");
    println!("    # Bash - detect if using PROMPT_COMMAND (modern prompt frameworks)");
    println!("    if [ -n \"$PROMPT_COMMAND\" ]; then");
    println!("        # Using a prompt framework like Starship, Oh My Bash, etc.");
    println!("        export SYFTBOX_OLD_PROMPT_COMMAND=\"$PROMPT_COMMAND\"");
    println!("        export PROMPT_COMMAND='echo -ne \"\\033[0m📦 (${{SYFTBOX_ENV_NAME}}) \"; '\"$PROMPT_COMMAND\"");
    println!("    else");
    println!("        # Traditional bash prompt");
    println!("        export SYFTBOX_OLD_PS1=\"$PS1\"");
    println!("        export PS1=\"\\[\\033[0m\\]📦 (${{SYFTBOX_ENV_NAME}}) ${{PS1}}\"");
    println!("    fi");
    println!("fi");

    // Set flag to refresh Powerlevel10k on next prompt (deferred to avoid instant prompt issues)
    println!("if typeset -f p10k >/dev/null 2>&1; then");
    println!("    export _SBENV_NEEDS_P10K_RELOAD=1");
    println!("fi");

    Ok(())
}

fn deactivate_environment(quiet: bool) -> Result<()> {
    if !quiet {
        println!("# Run this command to deactivate the environment:");
        println!("# eval \"$(sbenv deactivate)\"");
        println!();
    }

    println!("unset SYFTBOX_EMAIL");
    println!("unset SYFTBOX_DATA_DIR");
    println!("unset SYFTBOX_SERVER_URL");
    println!("unset SYFTBOX_CONFIG_PATH");
    println!("unset SYFTBOX_CLIENT_URL");
    println!("unset SYFTBOX_ENV_NAME");
    println!("unset SYFTBOX_BINARY");
    println!("unset SYFTBOX_VERSION");
    println!("unset SYFTBOX_BUILD_HASH");
    println!("unset SYFTBOX_BUILD_TARGET");

    // Restore VIRTUAL_ENV
    println!("if [ -n \"$SYFTBOX_OLD_VIRTUAL_ENV\" ]; then");
    println!("    export VIRTUAL_ENV=\"$SYFTBOX_OLD_VIRTUAL_ENV\"");
    println!("    unset SYFTBOX_OLD_VIRTUAL_ENV");
    println!("else");
    println!("    unset VIRTUAL_ENV");
    println!("fi");

    // Restore Powerlevel10k settings
    println!("if [ -n \"$ZSH_VERSION\" ]; then");
    println!("    if [ -n \"$SYFTBOX_OLD_P9K_CONTENT\" ]; then");
    println!(
        "        export POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION=\"$SYFTBOX_OLD_P9K_CONTENT\""
    );
    println!("        unset SYFTBOX_OLD_P9K_CONTENT");
    println!("    else");
    println!("        unset POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION");
    println!("    fi");
    println!("    if [ -n \"$SYFTBOX_OLD_P9K_VISUAL\" ]; then");
    println!(
        "        export POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION=\"$SYFTBOX_OLD_P9K_VISUAL\""
    );
    println!("        unset SYFTBOX_OLD_P9K_VISUAL");
    println!("    else");
    println!("        unset POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION");
    println!("    fi");
    println!("    unset POWERLEVEL9K_VIRTUALENV_SHOW_PYTHON_VERSION");
    println!("    unset POWERLEVEL9K_VIRTUALENV_SHOW_WITH_PYENV");
    println!("fi");

    // Restore PS1 or PROMPT_COMMAND for non-Powerlevel10k shells
    println!("# Restore bash prompt");
    println!("if [ -n \"$SYFTBOX_OLD_PROMPT_COMMAND\" ]; then");
    println!("    export PROMPT_COMMAND=\"$SYFTBOX_OLD_PROMPT_COMMAND\"");
    println!("    unset SYFTBOX_OLD_PROMPT_COMMAND");
    println!("elif [ -n \"$SYFTBOX_OLD_PS1\" ]; then");
    println!("    export PS1=\"$SYFTBOX_OLD_PS1\"");
    println!("    unset SYFTBOX_OLD_PS1");
    println!("fi");

    println!("unset SYFTBOX_ENV_ACTIVE");
    // No console I/O here to avoid conflicts with instant prompt

    Ok(())
}

fn remove_environment(path: Option<PathBuf>) -> Result<()> {
    let target_path = path.unwrap_or_else(|| env::current_dir().unwrap());
    let syftbox_dir = target_path.join(".syftbox");

    if !syftbox_dir.exists() {
        println!("{}", "No SyftBox environment found at this location".red());
        return Ok(());
    }

    let config_path = syftbox_dir.join("config.json");
    if let Ok(config) = load_config(&config_path) {
        println!("About to remove SyftBox environment:");
        println!("  Email: {}", config.email.cyan());
        println!("  Path: {}", target_path.display().to_string().cyan());
        println!();

        let confirmation = dialoguer::Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Are you sure you want to remove this environment?")
            .default(false)
            .interact()
            .context("Failed to read confirmation")?;

        if confirmation {
            unregister_environment(&target_path)?;
            fs::remove_dir_all(&syftbox_dir).context("Failed to remove .syftbox directory")?;
            // Remove marker file if present
            let marker = target_path.join(".sbenv");
            if marker.exists() {
                fs::remove_file(marker).ok();
            }
            println!("{}", "✅ SyftBox environment removed".green());
        } else {
            println!("{}", "Cancelled".yellow());
        }
    } else {
        println!("{}", "Invalid or corrupted environment".red());
    }

    Ok(())
}

fn activate_environment_to_file(path: &Path) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir).ok_or_else(|| {
        anyhow::anyhow!("No SyftBox environment found in current directory or parents")
    })?;

    let config = load_config(&config_path)?;

    // Ensure .sbenv marker exists for this environment
    let _ = ensure_marker_exists(&config_path, &config);

    let env_name = Path::new(&config.data_dir)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sbenv");

    let mut script = String::new();
    script.push_str(&format!("export SYFTBOX_EMAIL=\"{}\"\n", config.email));
    script.push_str(&format!(
        "export SYFTBOX_DATA_DIR=\"{}\"\n",
        config.data_dir
    ));
    script.push_str(&format!(
        "export SYFTBOX_SERVER_URL=\"{}\"\n",
        config.server_url
    ));
    script.push_str(&format!(
        "export SYFTBOX_CONFIG_PATH=\"{}\"\n",
        config_path.display()
    ));
    if let Some(url) = &config.client_url {
        script.push_str(&format!("export SYFTBOX_CLIENT_URL=\"{}\"\n", url));
    }
    let (bin_path, bin_ver) =
        resolve_binary_for_env(&config_path).unwrap_or((PathBuf::from("syftbox"), None));
    script.push_str(&format!(
        "export SYFTBOX_BINARY=\"{}\"\n",
        bin_path.display()
    ));
    if let Some(v) = bin_ver {
        script.push_str(&format!("export SYFTBOX_VERSION=\"{}\"\n", v));
    }
    script.push_str("export SYFTBOX_ENV_ACTIVE=\"1\"\n");
    script.push_str(&format!("export SYFTBOX_ENV_NAME=\"{}\"\n", env_name));

    script.push_str("if [ -n \"$ZSH_VERSION\" ]; then\n");
    script.push_str("    export SYFTBOX_OLD_PS1=\"$PS1\"\n");
    script.push_str("    export SYFTBOX_OLD_PROMPT=\"$PROMPT\"\n");
    script.push_str("    export SYFTBOX_OLD_RPROMPT=\"$RPROMPT\"\n");
    script.push_str(&format!(
        "    export PROMPT=\"📦 ({}) $PROMPT\"\n",
        env_name
    ));
    script.push_str(&format!("    export PS1=\"📦 ({}) $PS1\"\n", env_name));
    script.push_str("else\n");
    script.push_str("    export SYFTBOX_OLD_PS1=\"$PS1\"\n");
    script.push_str(&format!("    export PS1=\"📦 ({}) $PS1\"\n", env_name));
    script.push_str("fi\n");

    fs::write(path, script).context("Failed to write activation script")?;

    println!("Activation script written to: {}", path.display());
    println!("Run: source {}", path.display());

    Ok(())
}

fn get_shell_config_file() -> Result<PathBuf> {
    let shell = env::var("SHELL").unwrap_or_else(|_| String::from("/bin/bash"));
    let home = dirs::home_dir().context("Failed to get home directory")?;

    let config_file = if shell.contains("zsh") {
        home.join(".zshrc")
    } else if shell.contains("bash") {
        home.join(".bashrc")
    } else if shell.contains("fish") {
        home.join(".config").join("fish").join("config.fish")
    } else {
        // Default to bashrc
        home.join(".bashrc")
    };

    Ok(config_file)
}

fn check_shell_functions_installed(rc_file: &Path) -> Result<bool> {
    if !rc_file.exists() {
        return Ok(false);
    }

    let file = fs::File::open(rc_file)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        if line.contains("# SyftBox environment functions") || line.contains("sbenv()") {
            return Ok(true);
        }
    }

    Ok(false)
}

fn check_auto_activation_installed(rc_file: &Path) -> Result<bool> {
    if !rc_file.exists() {
        return Ok(false);
    }

    let file = fs::File::open(rc_file)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        if line.contains("_sbenv_auto_hook") || line.contains("Auto-activate SyftBox envs") {
            return Ok(true);
        }
    }

    Ok(false)
}

fn get_shell_functions() -> String {
    let mut functions = String::new();

    // Add P10k deferred reload handler
    functions.push_str(
        "# P10k deferred reload handler to avoid instant prompt issues
_sbenv_p10k_precmd() {
    if (( ${+functions[p10k]} )) && [[ -n $_SBENV_NEEDS_P10K_RELOAD ]]; then
        unset _SBENV_NEEDS_P10K_RELOAD
        p10k reload 2>/dev/null
    fi
}

# Add to precmd hooks if in ZSH
if [ -n \"$ZSH_VERSION\" ]; then
    if (( ${+functions[add-zsh-hook]} )); then
        autoload -Uz add-zsh-hook 2>/dev/null
        add-zsh-hook precmd _sbenv_p10k_precmd 2>/dev/null
    fi
fi

",
    );
    functions.push_str(
        "
# SyftBox environment functions
",
    );
    functions.push_str(
        "sbenv() {
",
    );
    functions.push_str("    case \"$1\" in\n");
    functions.push_str(
        "        activate)
",
    );
    functions.push_str("            eval \"$(command sbenv activate --quiet)\"\n");
    functions.push_str(
        "            # Fix Powerlevel10k prompt to show 📦 and email instead of 'Py'
",
    );
    functions
        .push_str("            if [ -n \"$ZSH_VERSION\" ] && [ -n \"$SYFTBOX_EMAIL\" ]; then\n");
    functions.push_str(
        "                export POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION=\"📦 $SYFTBOX_EMAIL\"\n",
    );
    functions.push_str(
        "                export POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION=''
",
    );
    functions.push_str(
        "                export POWERLEVEL9K_VIRTUALENV_SHOW_PYTHON_VERSION=false
",
    );
    functions.push_str(
        "                export POWERLEVEL9K_VIRTUALENV_SHOW_WITH_PYENV=false
",
    );
    functions.push_str(
        "                # Force P10k to rebuild its prompt cache
",
    );
    functions.push_str(
        "                unset _p9k__cached_p10k_param_sig 2>/dev/null
",
    );
    functions.push_str(
        "                # Defer P10k reload to avoid instant prompt issues
",
    );
    functions.push_str(
        "                if typeset -f p10k >/dev/null 2>&1; then
",
    );
    functions.push_str(
        "                    export _SBENV_NEEDS_P10K_RELOAD=1
",
    );
    functions.push_str(
        "                fi
",
    );
    functions.push_str(
        "            fi
",
    );
    functions.push_str(
        "            ;;
",
    );
    functions.push_str(
        "        deactivate)
",
    );
    functions.push_str("            eval \"$(command sbenv deactivate --quiet)\"\n");
    functions.push_str(
        "            # Reset P10k virtualenv display
",
    );
    functions.push_str("            if [ -n \"$ZSH_VERSION\" ]; then\n");
    functions.push_str(
        "                export POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION='${VIRTUAL_ENV:t}'
",
    );
    functions.push_str(
        "                export POWERLEVEL9K_VIRTUALENV_SHOW_PYTHON_VERSION=false
",
    );
    functions.push_str(
        "                unset _p9k__cached_p10k_param_sig 2>/dev/null
",
    );
    functions.push_str(
        "                # Defer P10k reload to avoid instant prompt issues
",
    );
    functions.push_str(
        "                if typeset -f p10k >/dev/null 2>&1; then
",
    );
    functions.push_str(
        "                    export _SBENV_NEEDS_P10K_RELOAD=1
",
    );
    functions.push_str(
        "                fi
",
    );
    functions.push_str(
        "            fi
",
    );
    functions.push_str(
        "            ;;
",
    );
    functions.push_str(
        "        *)
",
    );
    functions.push_str("            command sbenv \"$@\"\n");
    functions.push_str(
        "            ;;
",
    );
    functions.push_str(
        "    esac
",
    );
    functions.push_str(
        "}
",
    );
    functions.push('\n');
    functions.push_str(
        "# SyftBox environment aliases
",
    );
    functions.push_str(
        "alias sba='sbenv activate'
",
    );
    functions.push_str(
        "alias sbd='sbenv deactivate'
",
    );
    functions.push_str(
        "alias sbi='sbenv info'
",
    );
    functions
}

fn get_auto_activation_block() -> String {
    let mut s = String::new();
    s.push_str("# Auto-activate SyftBox envs when entering directories with a .sbenv marker\n");
    s.push_str("_sbenv_find_root() {\n");
    s.push_str("    local dir=\"$PWD\"\n");
    s.push_str("    while [ \"$dir\" != \"/\" ]; do\n");
    s.push_str("        if [ -f \"$dir/.sbenv\" ]; then\n");
    s.push_str("            echo \"$dir\"\n");
    s.push_str("            return 0\n");
    s.push_str("        fi\n");
    s.push_str("        dir=\"$(dirname \"$dir\")\"\n");
    s.push_str("    done\n");
    s.push_str("    return 1\n");
    s.push_str("}\n");
    s.push('\n');
    s.push_str("_sbenv_auto_hook() {\n");
    s.push_str("    local root\n");
    s.push_str("    root=\"$(_sbenv_find_root 2>/dev/null)\"\n");
    s.push_str("    if [ -n \"$root\" ]; then\n");
    s.push_str("        if [ \"$SBENV_AUTO_ACTIVE_ROOT\" != \"$root\" ]; then\n");
    s.push_str("            if [ -n \"$SYFTBOX_ENV_ACTIVE\" ]; then\n");
    s.push_str(
        "                SBENV_SUPPRESS_MESSAGES=1 eval \"$(command sbenv deactivate --quiet)\"\n",
    );
    s.push_str("            fi\n");
    s.push_str(
        "            SBENV_SUPPRESS_MESSAGES=1 eval \"$(command sbenv activate --quiet)\"\n",
    );
    s.push_str("            export SBENV_AUTO_ACTIVE_ROOT=\"$root\"\n");
    s.push_str("        fi\n");
    s.push_str("    else\n");
    s.push_str(
        "        if [ -n \"$SBENV_AUTO_ACTIVE_ROOT\" ] && [ -n \"$SYFTBOX_ENV_ACTIVE\" ]; then\n",
    );
    s.push_str(
        "            SBENV_SUPPRESS_MESSAGES=1 eval \"$(command sbenv deactivate --quiet)\"\n",
    );
    s.push_str("            unset SBENV_AUTO_ACTIVE_ROOT\n");
    s.push_str("        fi\n");
    s.push_str("    fi\n");
    s.push_str("}\n");
    s.push('\n');
    s.push_str("# Hook into ZSH bash-style directory change or Bash prompt\n");
    s.push_str("if [ -n \"$ZSH_VERSION\" ]; then\n");
    s.push_str("    typeset -ga chpwd_functions\n");
    s.push_str("    case \" ${chpwd_functions[@]} \" in *\\ _sbenv_auto_hook\\ *) ;; *) chpwd_functions+=(_sbenv_auto_hook) ;; esac\n");
    s.push_str(
        "    # Don't call _sbenv_auto_hook immediately - let it run on first directory change\n",
    );
    s.push_str("else\n");
    s.push_str("    if [ -z \"$SBENV_AUTO_PROMPT_HOOK\" ]; then\n");
    s.push_str("        export PROMPT_COMMAND=\"_sbenv_auto_hook; ${PROMPT_COMMAND}\"\n");
    s.push_str("        export SBENV_AUTO_PROMPT_HOOK=1\n");
    s.push_str("    fi\n");
    s.push_str("    # Don't call _sbenv_auto_hook immediately - let it run on first prompt\n");
    s.push_str("fi\n");
    s
}

fn install_shell_functions() -> Result<()> {
    let shell = env::var("SHELL").unwrap_or_else(|_| String::from("/bin/bash"));
    let shell_name = if shell.contains("zsh") {
        "ZSH"
    } else if shell.contains("bash") {
        "Bash"
    } else if shell.contains("fish") {
        "Fish"
    } else {
        "your shell"
    };

    let rc_file = get_shell_config_file()?;

    println!("{}", format!("🐚 Detected shell: {}", shell_name).cyan());
    println!(
        "📄 Configuration file: {}",
        rc_file.display().to_string().cyan()
    );
    println!();

    // Determine what needs to be added
    let already_funcs = check_shell_functions_installed(&rc_file)?;
    let already_auto = check_auto_activation_installed(&rc_file)?;

    if already_funcs && already_auto {
        println!(
            "{}",
            "✅ SyftBox shell functions and auto-activation are already installed!".green()
        );
        println!("All helpers are ready to use.");
        println!("If you haven't reloaded your shell config, run:");
        println!("  {}", format!("source {}", rc_file.display()).yellow());
        return Ok(());
    }

    let mut to_add = String::new();
    if !already_funcs {
        to_add.push_str(&get_shell_functions());
        to_add.push('\n');
    }
    if !already_auto {
        to_add.push_str(&get_auto_activation_block());
        to_add.push('\n');
    }

    // Show what will be added
    println!("The following will be added to your {} file:", shell_name);
    println!("{}", "─".repeat(50).dimmed());
    print!("{}", to_add.dimmed());
    println!("{}", "─".repeat(50).dimmed());
    println!();

    // Ask for confirmation
    let confirm = if atty::is(atty::Stream::Stdin) {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Add SyftBox functions to {}?", rc_file.display()))
            .default(true)
            .interact()?
    } else {
        println!("Non-interactive mode detected. Run with --manual flag to see installation instructions.");
        return Ok(());
    };

    if !confirm {
        println!("{}", "Installation cancelled.".yellow());
        println!("You can manually add the functions by running:");
        println!("  {}", "sbenv install-shell --manual".cyan());
        return Ok(());
    }

    // Create backup
    if rc_file.exists() {
        let backup_path = rc_file.with_extension("bak.sbenv");
        fs::copy(&rc_file, &backup_path)?;
        println!(
            "📦 Backup created: {}",
            backup_path.display().to_string().dimmed()
        );
    }

    // Append to rc file
    let mut existing_content = if rc_file.exists() {
        fs::read_to_string(&rc_file)?
    } else {
        String::new()
    };

    // Ensure there's a newline before our content
    if !existing_content.is_empty() && !existing_content.ends_with('\n') {
        existing_content.push('\n');
    }

    existing_content.push_str(&to_add);
    existing_content.push('\n');
    existing_content.push_str(&get_auto_activation_block());

    fs::write(&rc_file, existing_content)?;

    println!(
        "{}",
        "✅ SyftBox functions successfully installed!"
            .green()
            .bold()
    );
    println!();
    println!("To start using the new commands, either:");
    println!(
        "  1. Run: {}",
        format!("source {}", rc_file.display()).yellow()
    );
    println!("  2. Open a new terminal");
    println!();
    println!("Available commands:");
    println!(
        "  {} - Activate environment (no eval needed!)",
        "sbenv activate".cyan()
    );
    println!("  {} - Deactivate environment", "sbenv deactivate".cyan());
    println!("  {} - Activate (shortcut)", "sba".cyan());
    println!("  {} - Deactivate (shortcut)", "sbd".cyan());
    println!("  {} - Show info (shortcut)", "sbi".cyan());
    println!();
    println!(
        "{}",
        "⚠️  Important for Powerlevel10k users:".yellow().bold()
    );
    println!(
        "   Add this line at the END of your {} file:",
        rc_file.display()
    );
    println!();
    println!(
        "   {}",
        "(( ! ${+functions[p10k]} )) || p10k finalize".cyan()
    );
    println!();
    println!("   This prevents instant prompt errors with sbenv auto-activation.");

    Ok(())
}

fn check_login_status(config_path: &Path) -> Result<bool> {
    // Check if refresh_token exists in config
    let config = load_config(config_path)?;

    // If there's a refresh token, assume we're logged in
    // The daemon will fail if the token is invalid, and we'll handle that
    Ok(config.refresh_token.is_some())
}

fn prompt_and_login(config_path: &Path) -> Result<()> {
    // If this environment is in dev mode, do not attempt login
    if load_config(config_path)?.dev_mode {
        println!("{}", "Dev mode environment: skipping login.".yellow());
        return Ok(());
    }

    println!("{}", "You are not logged in to SyftBox.".yellow());

    let confirm = if atty::is(atty::Stream::Stdin) {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Would you like to log in now?")
            .default(true)
            .interact()?
    } else {
        println!("Cannot prompt for login in non-interactive mode.");
        println!(
            "Please run: {}",
            format!("syftbox -c {} login", config_path.display()).cyan()
        );
        return Err(anyhow::anyhow!("Not logged in"));
    };

    if !confirm {
        println!("{}", "Cannot start daemon without logging in.".red());
        println!(
            "Run: {}",
            format!("syftbox -c {} login", config_path.display()).cyan()
        );
        return Err(anyhow::anyhow!("Login required"));
    }

    // Save original config before login
    let original_config = load_config(config_path)?;

    println!("Logging in to SyftBox...");
    let (bin, _) = resolve_binary_for_env(config_path)?;
    let mut cmd = Command::new(bin);
    let status = cmd
        .args(["-c", config_path.to_str().unwrap(), "login"])
        .env("SYFTBOX_CONFIG", config_path.to_str().unwrap())
        .env("SYFTBOX_CLIENT_CONFIG_PATH", config_path.to_str().unwrap())
        // Enable auth bypass only in dev mode
        .envs(
            if load_config(config_path)?.dev_mode {
                Some(("SYFTBOX_AUTH_ENABLED", "0"))
            } else {
                None::<(&str, &str)>
            }
            .into_iter(),
        )
        .status()?;

    if !status.success() {
        return Err(anyhow::anyhow!("Login failed"));
    }

    // Restore original config values but keep the new refresh_token
    restore_config_after_login(config_path, &original_config)?;

    println!("{}", "✅ Login successful!".green());
    Ok(())
}

fn cleanup_orphaned_processes(config_path: &Path) -> Result<()> {
    // Check for any syftbox processes using this config file
    let config_path_str = config_path.to_str().unwrap();

    // Use pgrep to find syftbox processes
    let output = Command::new("pgrep").args(["-fl", "syftbox"]).output();

    if let Ok(output) = output {
        let processes = String::from_utf8_lossy(&output.stdout);
        for line in processes.lines() {
            // Check if this process is using our config file
            if line.contains(config_path_str) {
                // Extract PID from the line (first field)
                if let Some(pid_str) = line.split_whitespace().next() {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        println!(
                            "Found orphaned syftbox process (PID: {}) for this environment",
                            pid
                        );
                        println!("Killing orphaned process...");

                        // Try graceful kill first
                        Command::new("kill").arg(pid.to_string()).output()?;
                        thread::sleep(Duration::from_secs(2));

                        // Check if still running and force kill if needed
                        let check = Command::new("ps").args(["-p", &pid.to_string()]).output()?;
                        if check.status.success() {
                            println!("Force killing stubborn process...");
                            Command::new("kill")
                                .args(["-9", &pid.to_string()])
                                .output()?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn start_daemon(force: bool, skip_login_check: bool, daemon: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir)
        .ok_or_else(|| anyhow::anyhow!("No SyftBox environment found. Run 'sbenv init' first."))?;

    // Clean up any orphaned processes for this environment
    cleanup_orphaned_processes(&config_path)?;

    let mut config = load_config(&config_path)?;
    // Always use the environment directory for PID and logs, not config.data_dir
    // because syftbox login might change data_dir
    let env_dir = config_path.parent().unwrap().parent().unwrap();
    let syftbox_dir = env_dir.join(".syftbox");
    let pid_file = syftbox_dir.join("syftbox.pid");
    let log_file = syftbox_dir.join("daemon.log");

    // Check if already running (only relevant for daemon mode)
    if daemon && !force && pid_file.exists() {
        if let Ok(pid_str) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                // Check if process is actually running
                let check = Command::new("ps").args(["-p", &pid.to_string()]).output()?;

                if check.status.success() {
                    println!("{}", "SyftBox daemon is already running!".yellow());
                    println!("  PID: {}", pid.to_string().cyan());
                    println!("  Use {} to force restart", "sbenv start --force".yellow());
                    return Ok(());
                } else {
                    // Stale PID file
                    println!("Removing stale PID file...");
                    fs::remove_file(&pid_file)?;
                }
            }
        }
    }

    // Check if logged in (unless skipped or dev mode)
    // Only prompt if there's definitely no token
    let effective_skip_login = skip_login_check || config.dev_mode;
    if !effective_skip_login && !check_login_status(&config_path)? {
        prompt_and_login(&config_path)?;
        // Reload config after login
        config = load_config(&config_path)?;
    }

    // Prepare args and optionally set http addr if client_url is present (or derivable)
    let mut syftbox_args: Vec<String> = vec!["-c".into(), config_path.to_str().unwrap().into()];
    if daemon {
        syftbox_args.push("daemon".into());
    }
    if daemon {
        // Prefer config client_url, otherwise try derive from registry port
        let derived_url = if let Some(url) = &config.client_url {
            Some(url.clone())
        } else {
            // derive http://127.0.0.1:<port> from registry if available
            let registry = load_registry().unwrap_or(EnvRegistry {
                environments: HashMap::new(),
            });
            let env_dir = config_path
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_string_lossy()
                .to_string();
            let port = registry
                .environments
                .values()
                .find(|info| info.path == env_dir)
                .map(|info| info.port)
                .unwrap_or(0);
            if port > 0 {
                Some(format!("http://127.0.0.1:{}", port))
            } else {
                None
            }
        };

        if let Some(url) = derived_url {
            let http_addr_owned = url.strip_prefix("http://").unwrap_or(&url).to_string();
            syftbox_args.push("--http-addr".into());
            syftbox_args.push(http_addr_owned);
        }
    }

    if daemon {
        println!("{}", "Starting SyftBox daemon (background)...".green());
    } else {
        println!("{}", "Starting SyftBox (background)...".green());
    }
    println!("  Email: {}", config.email.cyan());
    // Determine client URL for display
    let client_url_display = if let Some(url) = &config.client_url {
        url.clone()
    } else {
        let registry = load_registry().unwrap_or(EnvRegistry {
            environments: HashMap::new(),
        });
        let env_dir = config_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let port = registry
            .environments
            .values()
            .find(|info| info.path == env_dir)
            .map(|info| info.port)
            .unwrap_or(0);
        if port > 0 {
            format!("http://127.0.0.1:{}", port)
        } else {
            "unknown".to_string()
        }
    };
    println!("  Client URL: {}", client_url_display.cyan());
    println!("  Data dir: {}", config.data_dir.cyan());
    println!("  Config: {}", config_path.display().to_string().cyan());
    if config.dev_mode {
        println!("  Mode  : {}", "dev".cyan());
    }

    // Create log file (both modes use the same log so 'sbenv logs' works)
    let log = fs::File::create(&log_file)?;

    // WORKAROUND: Temporarily rename global config if it exists
    // because syftbox ignores -c flag and always uses ~/.syftbox/config.json
    let home_config = dirs::home_dir()
        .unwrap()
        .join(".syftbox")
        .join("config.json");
    let home_config_backup = dirs::home_dir()
        .unwrap()
        .join(".syftbox")
        .join("config.json.sbenv_backup");
    let local_config_backup = config_path.with_extension("json.sbenv_local_backup");
    let mut restored_home_config = false;

    if home_config.exists() && home_config != config_path {
        println!("  Temporarily moving global config aside...");
        fs::rename(&home_config, &home_config_backup)?;
        // Backup our local config before copying it to global location
        fs::copy(&config_path, &local_config_backup)?;
        // Copy our config to the global location
        fs::copy(&config_path, &home_config)?;
        restored_home_config = true;
    }

    // Background execution using nohup for both modes; write output to log file
    let (bin, _) = resolve_binary_for_env(&config_path)?;
    let mut nohup = Command::new("nohup");
    let child = nohup
        .arg(bin.to_str().unwrap())
        .args(&syftbox_args)
        .env("SYFTBOX_CONFIG", config_path.to_str().unwrap())
        .env("SYFTBOX_CLIENT_CONFIG_PATH", config_path.to_str().unwrap())
        // Enable auth bypass only in dev mode
        .envs(
            if config.dev_mode {
                Some(("SYFTBOX_AUTH_ENABLED", "0"))
            } else {
                None::<(&str, &str)>
            }
            .into_iter(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
        .context("Failed to start syftbox in background. Is 'syftbox' installed?")?;

    let pid = child.id();

    // Save PID
    fs::write(&pid_file, pid.to_string())?;

    // Wait a moment and check if it's still running
    thread::sleep(Duration::from_secs(2));

    let check = Command::new("ps").args(["-p", &pid.to_string()]).output()?;

    // Restore the original global config if we moved it
    if restored_home_config {
        thread::sleep(Duration::from_millis(500)); // Give daemon time to read config
        fs::remove_file(&home_config).ok();
        fs::rename(&home_config_backup, &home_config)?;
        // Restore our local config from backup to preserve dev_mode and other fields
        if local_config_backup.exists() {
            fs::copy(&local_config_backup, &config_path)?;
            fs::remove_file(&local_config_backup)?;
        }
        println!("  Restored global config");
    }

    if check.status.success() {
        if daemon {
            println!(
                "{}",
                "✅ SyftBox daemon started successfully!".green().bold()
            );
        } else {
            println!("{}", "✅ SyftBox started in background".green().bold());
        }
        println!("  PID: {}", pid.to_string().cyan());
        println!("  Logs: {}", "sbenv logs".yellow());
        println!("  Status: {}", "sbenv status".yellow());
        println!("  Stop: {}", "sbenv stop".yellow());

        // Try to check HTTP API (if URL is available)
        if let Some(url) = &config.client_url {
            thread::sleep(Duration::from_secs(1));
            let api_check = Command::new("curl")
                .args([
                    "-s",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{http_code}",
                    &format!("{}/v1/status", url),
                ])
                .output();

            if let Ok(output) = api_check {
                let status_code = String::from_utf8_lossy(&output.stdout);
                if status_code == "200" || status_code == "401" {
                    println!("  API: {} Responding", "✓".green());
                }
            }
        }
    } else {
        fs::remove_file(&pid_file).ok();
        println!("{}", "❌ Failed to start daemon".red());
        println!("Check logs at: {}", log_file.display());
        return Err(anyhow::anyhow!("Daemon failed to start"));
    }

    Ok(())
}

fn stop_daemon() -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir)
        .ok_or_else(|| anyhow::anyhow!("No SyftBox environment found"))?;

    let _config = load_config(&config_path)?;
    // Always use the environment directory for PID file
    let env_dir = config_path.parent().unwrap().parent().unwrap();
    let pid_file = env_dir.join(".syftbox").join("syftbox.pid");

    // First, check for and clean up any orphaned processes for this environment
    cleanup_orphaned_processes(&config_path)?;

    if !pid_file.exists() {
        println!("{}", "No daemon PID file found".yellow());
        return Ok(());
    }

    let pid_str = fs::read_to_string(&pid_file)?;
    let pid = pid_str
        .trim()
        .parse::<u32>()
        .context("Invalid PID in file")?;

    // Check if process is running
    let check = Command::new("ps").args(["-p", &pid.to_string()]).output()?;

    if !check.status.success() {
        println!("Daemon not running (cleaning up stale PID)");
        fs::remove_file(&pid_file)?;
        return Ok(());
    }

    println!(
        "Stopping SyftBox daemon (PID: {})...",
        pid.to_string().cyan()
    );

    // Send SIGTERM
    Command::new("kill").arg(pid.to_string()).output()?;

    // Wait for graceful shutdown
    for i in 0..10 {
        thread::sleep(Duration::from_secs(1));
        let check = Command::new("ps").args(["-p", &pid.to_string()]).output()?;

        if !check.status.success() {
            println!("{}", "✅ SyftBox daemon stopped".green());
            fs::remove_file(&pid_file)?;
            return Ok(());
        }

        if i == 5 {
            println!("Daemon still running, sending force kill...");
            Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output()?;
        }
    }

    fs::remove_file(&pid_file)?;
    println!("{}", "✅ SyftBox daemon force stopped".green());
    Ok(())
}

fn show_daemon_status() -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir)
        .ok_or_else(|| anyhow::anyhow!("No SyftBox environment found"))?;

    let config = load_config(&config_path)?;

    // Ensure .sbenv marker exists for this environment
    let _ = ensure_marker_exists(&config_path, &config);
    // Always use the environment directory for PID file
    let env_dir = config_path.parent().unwrap().parent().unwrap();
    let pid_file = env_dir.join(".syftbox").join("syftbox.pid");

    if !pid_file.exists() {
        println!("{} No daemon found", "✗".red());
        println!("  Run {} to start", "sbenv start".yellow());
        return Ok(());
    }

    let pid_str = fs::read_to_string(&pid_file)?;
    let pid = pid_str
        .trim()
        .parse::<u32>()
        .context("Invalid PID in file")?;

    // Check if process is running
    let check = Command::new("ps").args(["-p", &pid.to_string()]).output()?;

    if !check.status.success() {
        println!("{} Daemon not running (stale PID: {})", "✗".red(), pid);
        fs::remove_file(&pid_file)?;
        return Ok(());
    }

    println!("{} SyftBox daemon running", "✓".green());
    println!("  PID: {}", pid.to_string().cyan());
    println!("  Email: {}", config.email.cyan());
    // Determine client URL for display (prefer config, fallback to registry)
    let client_url_display = if let Some(url) = &config.client_url {
        url.clone()
    } else {
        let registry = load_registry().unwrap_or(EnvRegistry {
            environments: HashMap::new(),
        });
        let env_dir = config_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let port = registry
            .environments
            .values()
            .find(|info| info.path == env_dir)
            .map(|info| info.port)
            .unwrap_or(0);
        if port > 0 {
            format!("http://127.0.0.1:{}", port)
        } else {
            "unknown".to_string()
        }
    };
    println!("  Client URL: {}", client_url_display.cyan());
    println!("  Data dir: {}", config.data_dir.cyan());

    // Check API
    if let Some(url) = config
        .client_url
        .as_ref()
        .or(Some(&client_url_display))
        .filter(|u| *u != "unknown")
    {
        let api_check = Command::new("curl")
            .args([
                "-s",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                &format!("{}/v1/status", url),
            ])
            .output();

        if let Ok(output) = api_check {
            let status_code = String::from_utf8_lossy(&output.stdout);
            if status_code == "200" || status_code == "401" {
                println!("  API: {} Responding", "✓".green());
            } else {
                println!("  API: {} Not responding (HTTP {})", "✗".red(), status_code);
            }
        } else {
            println!("  API: {} Cannot connect", "✗".red());
        }
    } else {
        println!("  API: {} URL not set in config", "–".dimmed());
    }

    Ok(())
}

fn show_daemon_logs(lines: Option<usize>, follow: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir)
        .ok_or_else(|| anyhow::anyhow!("No SyftBox environment found"))?;

    let _config = load_config(&config_path)?;
    // Always use the environment directory for log file
    let env_dir = config_path.parent().unwrap().parent().unwrap();
    let log_file = env_dir.join(".syftbox").join("daemon.log");

    if !log_file.exists() {
        println!("{}", "No log file found".yellow());
        println!("Start the daemon first with: {}", "sbenv start".cyan());
        return Ok(());
    }

    let mut args = vec![];
    let lines_str;

    if let Some(n) = lines {
        lines_str = format!("-{}", n);
        args.push(lines_str.as_str());
    } else if follow {
        args.push("-f");
    } else {
        args.push("-50");
    }

    args.push(log_file.to_str().unwrap());

    let status = Command::new("tail").args(&args).status()?;

    if !status.success() {
        return Err(anyhow::anyhow!("Failed to read log file"));
    }

    Ok(())
}

fn restart_daemon() -> Result<()> {
    println!("{}", "Restarting SyftBox daemon...".yellow());

    // Stop if running
    let _ = stop_daemon();

    thread::sleep(Duration::from_secs(1));

    // Start again
    start_daemon(false, false, true)
}

fn restore_config_after_login(config_path: &Path, original_config: &SyftBoxConfig) -> Result<()> {
    // Load the config that syftbox login modified
    let content = fs::read_to_string(config_path)?;
    let mut modified_config: serde_json::Value = serde_json::from_str(&content)?;

    // Restore original values but keep refresh_token
    if let Some(obj) = modified_config.as_object_mut() {
        obj.insert(
            "data_dir".to_string(),
            serde_json::Value::String(original_config.data_dir.clone()),
        );
        obj.insert(
            "email".to_string(),
            serde_json::Value::String(original_config.email.clone()),
        );
        obj.insert(
            "server_url".to_string(),
            serde_json::Value::String(original_config.server_url.clone()),
        );
        if let Some(url) = &original_config.client_url {
            obj.insert(
                "client_url".to_string(),
                serde_json::Value::String(url.clone()),
            );
        } else {
            obj.remove("client_url");
        }
        // Preserve dev_mode flag
        obj.insert(
            "dev_mode".to_string(),
            serde_json::Value::Bool(original_config.dev_mode),
        );
        // Keep the refresh_token from login
    }

    // Write back the fixed config
    let fixed_json = serde_json::to_string_pretty(&modified_config)?;
    fs::write(config_path, fixed_json)?;

    println!("  Restored environment config settings");
    Ok(())
}

fn login_to_syftbox() -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir)
        .ok_or_else(|| anyhow::anyhow!("No SyftBox environment found. Run 'sbenv init' first."))?;

    let original_config = load_config(&config_path)?;

    if original_config.dev_mode {
        println!("{}", "Dev mode environment: skipping login.".yellow());
        return Ok(());
    }

    println!("{}", "Logging in to SyftBox...".green());
    println!("  Email: {}", original_config.email.cyan());
    println!("  Server: {}", original_config.server_url.cyan());
    println!("  Config: {}", config_path.display().to_string().cyan());
    println!();

    let (bin, _) = resolve_binary_for_env(&config_path)?;
    let mut cmd = Command::new(bin);
    let status = cmd
        .args(["-c", config_path.to_str().unwrap(), "login"])
        .env("SYFTBOX_CONFIG", config_path.to_str().unwrap())
        .env("SYFTBOX_CLIENT_CONFIG_PATH", config_path.to_str().unwrap())
        // Enable auth bypass only in dev mode
        .envs(
            if original_config.dev_mode {
                Some(("SYFTBOX_AUTH_ENABLED", "0"))
            } else {
                None::<(&str, &str)>
            }
            .into_iter(),
        )
        .status()
        .context("Failed to run syftbox login. Is 'syftbox' installed?")?;

    if status.success() {
        // Restore original config values but keep the new refresh_token
        restore_config_after_login(&config_path, &original_config)?;

        println!();
        println!("{}", "✅ Login successful!".green().bold());
        println!(
            "You can now run {} to start the daemon",
            "sbenv start".yellow()
        );
    } else {
        return Err(anyhow::anyhow!("Login failed"));
    }

    Ok(())
}

fn list_environments() -> Result<()> {
    let registry = load_registry()?;

    if registry.environments.is_empty() {
        println!("{}", "No SyftBox environments registered yet.".yellow());
        println!("Use {} to create one.", "sbenv init".cyan());
        return Ok(());
    }

    println!("{}", "📦 SyftBox Environments".bold());
    println!();

    // Collect and sort by email (case-insensitive)
    let mut envs: Vec<&EnvInfo> = registry.environments.values().collect();
    envs.sort_by(|a, b| a.email.to_lowercase().cmp(&b.email.to_lowercase()));

    for info in envs {
        let path = Path::new(&info.path);
        let exists = path.join(".syftbox").exists();
        let status = if exists { "✅".green() } else { "❌".red() };
        let top_name = Path::new(&info.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?");
        let dev_label = if info.dev_mode { " - DEV" } else { "" };

        // Primary line: status + email [ - DEV ] (top folder name)
        println!(
            "  {} {}{} ({})",
            status,
            info.email.cyan(),
            dev_label,
            top_name
        );
        println!("     Path : {}", info.path);
        println!("     Port : {}", info.port);
        if !info.server_url.is_empty() {
            println!("     Server: {}", info.server_url);
        }
        if let Some(b) = &info.binary {
            println!("     Bin  : {}", b);
        }
        if let Some(v) = &info.binary_version {
            println!("     Ver  : {}", v);
        }
        if let Some(h) = &info.binary_hash {
            println!("     Hash : {}", h);
        }
        if info.binary_os.is_some() || info.binary_arch.is_some() {
            println!(
                "     Target: {}/{}",
                info.binary_os.as_deref().unwrap_or("?"),
                info.binary_arch.as_deref().unwrap_or("?")
            );
        }

        // Show process info (last known PID and whether it's active)
        let pid_file = path.join(".syftbox").join("syftbox.pid");
        if pid_file.exists() {
            match fs::read_to_string(&pid_file) {
                Ok(pid_str) => {
                    let pid_str = pid_str.trim().to_string();
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        let check = Command::new("ps")
                            .args(["-p", &pid.to_string()])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                        let running = matches!(check, Ok(s) if s.success());
                        if running {
                            println!("     PID  : {} (active)", pid.to_string().cyan());
                        } else {
                            println!("     PID  : {} (stale)", pid.to_string().yellow());
                        }
                    } else {
                        println!("     PID  : {}", "invalid".red());
                    }
                }
                Err(_) => {
                    println!("     PID  : {}", "unreadable".red());
                }
            }
        } else {
            println!("     PID  : {}", "-".dimmed());
        }
        println!();
    }

    Ok(())
}

fn update_environment(server_url: Option<String>, dev: Option<bool>) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir).ok_or_else(|| {
        anyhow::anyhow!("No SyftBox environment found in current directory or parents")
    })?;

    let mut config = load_config(&config_path)?;

    let mut changed = false;
    if let Some(url) = server_url {
        if config.server_url != url {
            config.server_url = url;
            changed = true;
        }
    }
    if let Some(dev_mode) = dev {
        if config.dev_mode != dev_mode {
            config.dev_mode = dev_mode;
            changed = true;
        }
    }

    if !changed {
        println!("No changes specified. Use --server_url or --dev true/false.");
        return Ok(());
    }

    // Save updated config
    let config_json =
        serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    fs::write(&config_path, config_json).context("Failed to write config file")?;

    // Update environment registry (path is env dir of config)
    let env_dir = config_path.parent().unwrap().parent().unwrap();
    register_environment(env_dir, &config)?;

    println!("{}", "✅ Environment updated".green().bold());
    println!("  Email : {}", config.email.cyan());
    println!("  Server: {}", config.server_url.cyan());
    println!(
        "  Dev   : {}",
        if config.dev_mode { "true" } else { "false" }
    );

    Ok(())
}

// Update check structs
#[derive(Debug, Deserialize)]
struct CratesApiResponse {
    #[serde(rename = "crate")]
    crate_info: CrateInfo,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    max_version: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
}

enum InstallMethod {
    Cargo,
    Binary,
}

fn get_current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Invalid current version")
}

fn detect_install_method() -> Result<InstallMethod> {
    let exe_path = env::current_exe().context("Failed to get current executable path")?;
    let exe_path_str = exe_path.to_string_lossy();

    if exe_path_str.contains(".cargo")
        || exe_path_str.contains("target/release")
        || exe_path_str.contains("target/debug")
    {
        Ok(InstallMethod::Cargo)
    } else {
        Ok(InstallMethod::Binary)
    }
}

async fn check_crates_io() -> Result<Option<Version>> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://crates.io/api/v1/crates/sbenv")
        .header("User-Agent", "sbenv-cli")
        .send()
        .await
        .context("Failed to check crates.io")?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let api_response: CratesApiResponse = response
        .json()
        .await
        .context("Failed to parse crates.io response")?;

    let latest_version = Version::parse(&api_response.crate_info.max_version)
        .context("Invalid version from crates.io")?;

    Ok(Some(latest_version))
}

async fn check_github() -> Result<Option<Version>> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/openmined/sbenv/releases/latest")
        .header("User-Agent", "sbenv-cli")
        .send()
        .await
        .context("Failed to check GitHub releases")?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let release: GithubRelease = response
        .json()
        .await
        .context("Failed to parse GitHub response")?;

    let version_str = release.tag_name.trim_start_matches('v');
    let latest_version = Version::parse(version_str).context("Invalid version from GitHub")?;

    Ok(Some(latest_version))
}

async fn check_for_updates() -> Result<Option<Version>> {
    let current = get_current_version();

    let crates_version = check_crates_io().await.ok().flatten();
    let github_version = check_github().await.ok().flatten();

    let latest = match (crates_version, github_version) {
        (Some(c), Some(g)) => Some(if c > g { c } else { g }),
        (Some(c), None) => Some(c),
        (None, Some(g)) => Some(g),
        (None, None) => None,
    };

    if let Some(ref version) = latest {
        if version > &current {
            return Ok(Some(version.clone()));
        }
    }

    Ok(None)
}

async fn update_via_cargo() -> Result<()> {
    println!("Updating via cargo install...");

    let output = Command::new("cargo")
        .args(["install", "sbenv", "--force"])
        .output()
        .context("Failed to run cargo install")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("cargo install failed: {}", stderr);
    }

    Ok(())
}

async fn update_via_self_update(new_version: &Version) -> Result<()> {
    println!("Updating via direct binary download...");

    let status = self_update::backends::github::Update::configure()
        .repo_owner("OpenMined")
        .repo_name("sbenv")
        .bin_name("sbenv")
        .target_version_tag(&format!("v{}", new_version))
        .show_download_progress(true)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()
        .context("Failed to build self-updater")?
        .update()
        .context("Failed to perform self-update")?;

    if let self_update::Status::Updated(_) = status {}

    Ok(())
}

async fn perform_update(new_version: &Version) -> Result<()> {
    println!("\n{} Updating sbenv...", "🔄".cyan());

    let install_method = detect_install_method()?;

    match install_method {
        InstallMethod::Cargo => update_via_cargo().await?,
        InstallMethod::Binary => update_via_self_update(new_version).await?,
    }

    println!(
        "\n{} {} {} {}!",
        "✨".green(),
        "Successfully updated to version".green().bold(),
        new_version.to_string().green().bold(),
        "".green().bold()
    );

    Ok(())
}

async fn self_update_sbenv_async(force: bool) -> Result<()> {
    println!("{}", "Checking for updates...".cyan());

    let current = get_current_version();
    println!("Current version: {}", current.to_string().cyan());

    match check_for_updates().await? {
        Some(new_version) => {
            println!(
                "\n{} {} {}",
                "✨".green(),
                "New version available:".green().bold(),
                new_version.to_string().green().bold()
            );

            let confirm = if force {
                true
            } else {
                Confirm::new()
                    .with_prompt(format!("Upgrade from {} to {}?", current, new_version))
                    .default(true)
                    .interact()
                    .context("Failed to get user confirmation")?
            };

            if confirm {
                perform_update(&new_version).await?;
            } else {
                println!("Update cancelled.");
            }
        }
        None => {
            println!(
                "{} {}",
                "✓".green(),
                "You're already on the latest version!".green()
            );
        }
    }

    Ok(())
}

fn self_update_sbenv(force: bool) -> Result<()> {
    // Create a runtime for the async operations
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(self_update_sbenv_async(force))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Init {
            email,
            server_url,
            dev,
            binary,
            quiet,
        }) => {
            init_environment_with_binary(
                email.clone(),
                server_url.clone(),
                *dev,
                binary.clone(),
                *quiet,
            )?;
        }
        Some(Commands::Info) => {
            show_info()?;
        }
        Some(Commands::Activate { write_to, quiet }) => {
            if let Some(path) = write_to {
                activate_environment_to_file(path)?;
            } else {
                activate_environment(*quiet)?;
            }
        }
        Some(Commands::Deactivate { quiet }) => {
            deactivate_environment(*quiet)?;
        }
        Some(Commands::Remove { path }) => {
            remove_environment(path.clone())?;
        }
        Some(Commands::Edit {
            server_url,
            dev,
            binary,
        }) => {
            update_environment(server_url.clone(), *dev)?;
            if let Some(bin_spec) = binary.clone() {
                // Update binary for current env and save to registry; also update global default
                let current_dir = env::current_dir().context("Failed to get current directory")?;
                let config_path = find_syftbox_config(&current_dir).ok_or_else(|| {
                    anyhow::anyhow!("No SyftBox environment found in current directory or parents")
                })?;
                let config = load_config(&config_path)?;
                let env_dir = config_path.parent().unwrap().parent().unwrap();
                let env_key = generate_env_key(env_dir, &config.email);
                let (p, v) = resolve_or_install_syftbox(&bin_spec)?;
                let mut registry = load_registry()?;
                if let Some(info) = registry.environments.get_mut(&env_key) {
                    info.binary = Some(p.to_string_lossy().to_string());
                    info.binary_version = v.clone();
                }
                save_registry(&registry)?;
                let mut gc = load_global_config();
                gc.default_binary = Some(bin_spec);
                let _ = save_global_config(&gc);
                println!(
                    "{}",
                    "✅ Updated syftbox binary for this environment".green()
                );
            }
        }
        Some(Commands::InstallShell { manual }) => {
            if *manual {
                println!("# Add these functions to your shell configuration:");
                println!("# For ZSH: add to ~/.zshrc");
                println!("# For Bash: add to ~/.bashrc");
                print!("{}", get_shell_functions());
                println!();
                print!("{}", get_auto_activation_block());
                println!();
                println!("After adding these functions, restart your shell or run:");
                println!("  source ~/.zshrc  # for ZSH");
                println!("  source ~/.bashrc # for Bash");
            } else {
                install_shell_functions()?;
            }
        }
        Some(Commands::Start {
            force,
            skip_login_check,
            daemon,
        }) => {
            start_daemon(*force, *skip_login_check, *daemon)?;
        }
        Some(Commands::Stop) => {
            stop_daemon()?;
        }
        Some(Commands::Status) => {
            show_daemon_status()?;
        }
        Some(Commands::Restart) => {
            restart_daemon()?;
        }
        Some(Commands::Logs { lines, follow }) => {
            show_daemon_logs(*lines, *follow)?;
        }
        Some(Commands::Login) => {
            login_to_syftbox()?;
        }
        Some(Commands::List) => {
            list_environments()?;
        }
        Some(Commands::Update { force }) => {
            self_update_sbenv(*force)?;
        }
        None => {
            if env::var("SYFTBOX_ENV_ACTIVE").is_ok() {
                show_info()?;
            } else {
                println!("{}", "SyftBox Env (sbenv) - virtualenv for SyftBox".bold());
                println!();
                println!("No active environment. Commands available:");
                println!(
                    "  {} - Initialize a new environment in current directory",
                    "sbenv init".yellow()
                );
                println!(
                    "  {} - Show current environment info",
                    "sbenv info".yellow()
                );
                println!("  {} - Activate environment", "sbenv activate".yellow());
                println!("  {} - Deactivate environment", "sbenv deactivate".yellow());
                println!("  {} - Remove an environment", "sbenv remove".yellow());
                println!(
                    "  {} - Install shell functions for easier use",
                    "sbenv install-shell".yellow()
                );
                println!();
                println!("Daemon commands:");
                println!("  {} - Start SyftBox daemon", "sbenv start".yellow());
                println!("  {} - Stop daemon", "sbenv stop".yellow());
                println!("  {} - Check daemon status", "sbenv status".yellow());
                println!("  {} - View daemon logs", "sbenv logs".yellow());
                println!("  {} - Restart daemon", "sbenv restart".yellow());
                println!("  {} - Login to SyftBox", "sbenv login".yellow());
                println!();
                println!("Use {} for more information", "sbenv --help".cyan());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Use a mutex to ensure tests that modify HOME don't run concurrently
    static HOME_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_find_available_port_with_empty_registry() {
        let _guard = HOME_MUTEX.lock().unwrap();
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let original_home = env::var("HOME").ok();
        env::set_var("HOME", temp_dir.path());

        // Test finding a port when registry is empty
        let port = find_available_port().unwrap();
        assert!((7939..=7999).contains(&port));

        // Restore original HOME
        if let Some(home) = original_home {
            env::set_var("HOME", home);
        }
    }

    #[test]
    fn test_find_available_port_with_used_ports() {
        let _guard = HOME_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let original_home = env::var("HOME").ok();
        env::set_var("HOME", temp_dir.path());

        // Create a registry with some used ports
        let mut registry = EnvRegistry {
            environments: HashMap::new(),
        };

        // Add environments with specific ports
        for i in 0..5 {
            let env_info = EnvInfo {
                path: format!("/test/path{}", i),
                email: format!("test{}@example.com", i),
                port: 7940 + i as u16,
                name: format!("test{}", i),
                server_url: "https://test.server".to_string(),
                dev_mode: false,
                binary: None,
                binary_version: None,
                binary_hash: None,
                binary_os: None,
                binary_arch: None,
            };
            registry.environments.insert(format!("test{}", i), env_info);
        }

        save_registry(&registry).unwrap();

        // Find an available port
        let port = find_available_port().unwrap();

        // Verify the port is in range and not in the used ports
        assert!((7939..=7999).contains(&port));
        let used_ports: Vec<u16> = (7940..7945).collect();
        assert!(!used_ports.contains(&port));

        if let Some(home) = original_home {
            env::set_var("HOME", home);
        }
    }

    #[test]
    fn test_register_and_unregister_environment() {
        let _guard = HOME_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let original_home = env::var("HOME").ok();
        env::set_var("HOME", temp_dir.path());

        let test_path = temp_dir.path().join("test_env");
        fs::create_dir(&test_path).unwrap();

        let config = SyftBoxConfig {
            data_dir: test_path.to_string_lossy().to_string(),
            email: "test@example.com".to_string(),
            server_url: "https://test.server".to_string(),
            client_url: Some("http://127.0.0.1:7950".to_string()),
            client_token: None,
            refresh_token: None,
            dev_mode: false,
        };

        // Register environment
        register_environment(&test_path, &config).unwrap();

        // Verify it was registered using the correct key
        let registry = load_registry().unwrap();
        let env_key = generate_env_key(&test_path, "test@example.com");
        assert!(registry.environments.contains_key(&env_key));
        let env_info = registry.environments.get(&env_key).unwrap();
        assert_eq!(env_info.email, "test@example.com");
        assert_eq!(env_info.port, 7950);

        // Unregister environment
        unregister_environment(&test_path).unwrap();

        // Verify it was removed
        let registry = load_registry().unwrap();
        assert!(!registry.environments.contains_key(&env_key));

        if let Some(home) = original_home {
            env::set_var("HOME", home);
        }
    }

    #[test]
    fn test_load_registry_creates_empty_if_not_exists() {
        let _guard = HOME_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let original_home = env::var("HOME").ok();
        env::set_var("HOME", temp_dir.path());

        // Load registry when it doesn't exist
        let registry = load_registry().unwrap();
        assert!(registry.environments.is_empty());

        if let Some(home) = original_home {
            env::set_var("HOME", home);
        }
    }

    #[test]
    fn test_get_used_ports() {
        let _guard = HOME_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let original_home = env::var("HOME").ok();
        env::set_var("HOME", temp_dir.path());

        let mut registry = EnvRegistry {
            environments: HashMap::new(),
        };

        // Add test environments
        registry.environments.insert(
            "env1".to_string(),
            EnvInfo {
                path: "/path1".to_string(),
                email: "test1@example.com".to_string(),
                port: 7940,
                name: "env1".to_string(),
                server_url: "https://test.server".to_string(),
                dev_mode: false,
                binary: None,
                binary_version: None,
                binary_hash: None,
                binary_os: None,
                binary_arch: None,
            },
        );
        registry.environments.insert(
            "env2".to_string(),
            EnvInfo {
                path: "/path2".to_string(),
                email: "test2@example.com".to_string(),
                port: 7945,
                name: "env2".to_string(),
                server_url: "https://test.server".to_string(),
                dev_mode: false,
                binary: None,
                binary_version: None,
                binary_hash: None,
                binary_os: None,
                binary_arch: None,
            },
        );

        save_registry(&registry).unwrap();

        // Get used ports
        let used_ports = get_used_ports().unwrap();
        assert_eq!(used_ports.len(), 2);
        assert!(used_ports.contains(&7940));
        assert!(used_ports.contains(&7945));

        if let Some(home) = original_home {
            env::set_var("HOME", home);
        }
    }

    #[test]
    fn test_parse_port_from_client_url() {
        let config = SyftBoxConfig {
            data_dir: "/test".to_string(),
            email: "test@example.com".to_string(),
            server_url: "https://test.server".to_string(),
            client_url: Some("http://127.0.0.1:7950".to_string()),
            client_token: None,
            refresh_token: None,
            dev_mode: false,
        };

        let port = config
            .client_url
            .as_deref()
            .and_then(|u| u.rsplit(':').next())
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap();

        assert_eq!(port, 7950);
    }

    #[test]
    fn test_parse_port_from_localhost_url() {
        let config = SyftBoxConfig {
            data_dir: "/test".to_string(),
            email: "test@example.com".to_string(),
            server_url: "https://test.server".to_string(),
            client_url: Some("http://localhost:8080".to_string()),
            client_token: None,
            refresh_token: None,
            dev_mode: false,
        };

        let port = config
            .client_url
            .as_deref()
            .and_then(|u| u.rsplit(':').next())
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap();

        assert_eq!(port, 8080);
    }

    #[test]
    fn test_registry_persistence() {
        let _guard = HOME_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let original_home = env::var("HOME").ok();
        env::set_var("HOME", temp_dir.path());

        // Create and save a registry
        let mut registry = EnvRegistry {
            environments: HashMap::new(),
        };
        registry.environments.insert(
            "persistent_env".to_string(),
            EnvInfo {
                path: "/persistent/path".to_string(),
                email: "persist@example.com".to_string(),
                port: 7960,
                name: "persistent_env".to_string(),
                server_url: "https://test.server".to_string(),
                dev_mode: false,
                binary: None,
                binary_version: None,
                binary_hash: None,
                binary_os: None,
                binary_arch: None,
            },
        );
        save_registry(&registry).unwrap();

        // Load it back and verify
        let loaded = load_registry().unwrap();
        assert_eq!(loaded.environments.len(), 1);
        assert!(loaded.environments.contains_key("persistent_env"));

        let env = loaded.environments.get("persistent_env").unwrap();
        assert_eq!(env.email, "persist@example.com");
        assert_eq!(env.port, 7960);

        if let Some(home) = original_home {
            env::set_var("HOME", home);
        }
    }
}
