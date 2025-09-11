use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct SyftBoxConfig {
    data_dir: String,
    email: String,
    server_url: String,
    client_url: String,
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
        /// SyftBox server URL
        #[arg(short, long, default_value = "https://syftbox.net")]
        server_url: String,
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
    /// List all SyftBox environments
    List,
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

fn init_environment(email: Option<String>, server_url: String) -> Result<()> {
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
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Email address")
            .interact_text()
            .context("Failed to read email input")?
    };

    let mut rng = rand::thread_rng();
    let port = rng.gen_range(7939..=7999);
    let client_url = format!("http://127.0.0.1:{}", port);

    let config = SyftBoxConfig {
        data_dir: current_dir.to_string_lossy().to_string(),
        email: email.clone(),
        server_url: server_url.clone(),
        client_url: client_url.clone(),
    };

    fs::create_dir_all(&syftbox_dir).context("Failed to create .syftbox directory")?;

    let config_path = syftbox_dir.join("config.json");
    let config_json =
        serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    fs::write(&config_path, config_json).context("Failed to write config file")?;

    println!("{}", "✅ SyftBox environment initialized!".green().bold());
    println!();
    println!("📧 Email: {}", email.cyan());
    println!("🌐 Server: {}", server_url.cyan());
    println!("📁 Data dir: {}", current_dir.display().to_string().cyan());
    println!("🔌 Client port: {}", port.to_string().cyan());
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

    let port = config.client_url.rsplit(':').next().unwrap_or("unknown");

    println!("{}", "📦 SyftBox Environment Info".green().bold());
    println!();
    println!("📧 Email: {}", config.email.cyan());
    println!("🌐 Server URL: {}", config.server_url.cyan());
    println!("📁 Data dir: {}", config.data_dir.cyan());
    println!("🔌 Client port: {}", port.cyan());
    println!(
        "📄 Config path: {}",
        config_path.display().to_string().cyan()
    );

    Ok(())
}

fn activate_environment(quiet: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let config_path = find_syftbox_config(&current_dir).ok_or_else(|| {
        anyhow::anyhow!("No SyftBox environment found in current directory or parents")
    })?;

    let config = load_config(&config_path)?;

    if !quiet {
        println!("# Run this command to activate the environment:");
        println!("# eval \"$(sbenv activate)\"");
        println!();
    }

    println!("export SYFTBOX_EMAIL=\"{}\"", config.email);
    println!("export SYFTBOX_DATA_DIR=\"{}\"", config.data_dir);
    println!("export SYFTBOX_SERVER_URL=\"{}\"", config.server_url);
    println!("export SYFTBOX_CONFIG_PATH=\"{}\"", config_path.display());
    println!("export SYFTBOX_CLIENT_URL=\"{}\"", config.client_url);
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
    println!("        export SYFTBOX_OLD_P9K_VISUAL=\"${{POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION}}\"");
    println!("        # Override to show box icon and email without 'Py'");
    println!("        export POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION='📦 {}'", config.email);
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
    println!("    # Bash");
    println!("    export SYFTBOX_OLD_PS1=\"$PS1\"");
    println!("    export PS1=\"📦 ($SYFTBOX_ENV_NAME) $PS1\"");
    println!("fi");

    println!("echo \"SyftBox environment activated: {}\" >&2", config.email);

    // Force Powerlevel10k to refresh if it's running
    println!("if typeset -f _p9k_precmd >/dev/null 2>&1; then");
    println!("    _p9k_precmd");
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
    println!("        export POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION=\"$SYFTBOX_OLD_P9K_VISUAL\"");
    println!("        unset SYFTBOX_OLD_P9K_VISUAL");
    println!("    else");
    println!("        unset POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION");
    println!("    fi");
    println!("    unset POWERLEVEL9K_VIRTUALENV_SHOW_PYTHON_VERSION");
    println!("    unset POWERLEVEL9K_VIRTUALENV_SHOW_WITH_PYENV");
    println!("fi");

    // Restore PS1 for non-Powerlevel10k shells
    println!("if [ -n \"$SYFTBOX_OLD_PS1\" ]; then");
    println!("    export PS1=\"$SYFTBOX_OLD_PS1\"");
    println!("    unset SYFTBOX_OLD_PS1");
    println!("fi");

    println!("unset SYFTBOX_ENV_ACTIVE");
    println!("echo \"SyftBox environment deactivated\" >&2");

    Ok(())
}

fn list_environments() -> Result<()> {
    let home_dir = dirs::home_dir().context("Failed to get home directory")?;
    let mut environments = Vec::new();

    fn find_environments(dir: &Path, environments: &mut Vec<PathBuf>, depth: usize) {
        if depth > 5 {
            return;
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let config_path = path.join(".syftbox").join("config.json");
                    if config_path.exists() {
                        environments.push(path.clone());
                    }
                    if path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| !n.starts_with('.'))
                        .unwrap_or(false)
                    {
                        find_environments(&path, environments, depth + 1);
                    }
                }
            }
        }
    }

    println!("{}", "🔍 Searching for SyftBox environments...".yellow());
    find_environments(&home_dir, &mut environments, 0);

    if let Ok(current_dir) = env::current_dir() {
        if !current_dir.starts_with(&home_dir) {
            find_environments(&current_dir, &mut environments, 0);
        }
    }

    if environments.is_empty() {
        println!("{}", "No SyftBox environments found".red());
    } else {
        println!(
            "{}",
            format!("Found {} environment(s):", environments.len())
                .green()
                .bold()
        );
        println!();

        for env_path in environments {
            let config_path = env_path.join(".syftbox").join("config.json");
            match load_config(&config_path) {
                Ok(config) => {
                    println!("📦 {}", env_path.display().to_string().cyan());
                    println!("   Email: {}", config.email);
                    println!("   Server: {}", config.server_url);
                    let port = config.client_url.rsplit(':').next().unwrap_or("unknown");
                    println!("   Port: {}", port);
                    println!();
                }
                Err(_) => {
                    println!(
                        "📦 {} {}",
                        env_path.display().to_string().cyan(),
                        "(invalid config)".red()
                    );
                    println!();
                }
            }
        }
    }

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
            fs::remove_dir_all(&syftbox_dir).context("Failed to remove .syftbox directory")?;
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
    script.push_str(&format!(
        "export SYFTBOX_CLIENT_URL=\"{}\"\n",
        config.client_url
    ));
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

fn get_shell_functions() -> String {
    let mut functions = String::new();
    functions.push_str("\n# SyftBox environment functions\n");
    functions.push_str("sbenv() {\n");
    functions.push_str("    case \"$1\" in\n");
    functions.push_str("        activate)\n");
    functions.push_str("            eval \"$(command sbenv activate --quiet)\"\n");
    functions.push_str("            # Fix Powerlevel10k prompt to show 📦 and email instead of 'Py'\n");
    functions.push_str("            if [[ -n \"$ZSH_VERSION\" ]] && [[ -n \"$SYFTBOX_EMAIL\" ]]; then\n");
    functions.push_str("                export POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION=\"📦 $SYFTBOX_EMAIL\"\n");
    functions.push_str("                export POWERLEVEL9K_VIRTUALENV_VISUAL_IDENTIFIER_EXPANSION=''\n");
    functions.push_str("                export POWERLEVEL9K_VIRTUALENV_SHOW_PYTHON_VERSION=false\n");
    functions.push_str("                export POWERLEVEL9K_VIRTUALENV_SHOW_WITH_PYENV=false\n");
    functions.push_str("                # Force P10k to rebuild its prompt cache\n");
    functions.push_str("                unset _p9k__cached_p10k_param_sig 2>/dev/null\n");
    functions.push_str("                if typeset -f p10k >/dev/null 2>&1; then\n");
    functions.push_str("                    p10k reload 2>/dev/null\n");
    functions.push_str("                elif typeset -f _p9k_precmd >/dev/null 2>&1; then\n");
    functions.push_str("                    _p9k_precmd\n");
    functions.push_str("                fi\n");
    functions.push_str("            fi\n");
    functions.push_str("            ;;\n");
    functions.push_str("        deactivate)\n");
    functions.push_str("            eval \"$(command sbenv deactivate --quiet)\"\n");
    functions.push_str("            # Reset P10k virtualenv display\n");
    functions.push_str("            if [[ -n \"$ZSH_VERSION\" ]]; then\n");
    functions.push_str("                export POWERLEVEL9K_VIRTUALENV_CONTENT_EXPANSION='${VIRTUAL_ENV:t}'\n");
    functions.push_str("                export POWERLEVEL9K_VIRTUALENV_SHOW_PYTHON_VERSION=false\n");
    functions.push_str("                unset _p9k__cached_p10k_param_sig 2>/dev/null\n");
    functions.push_str("                if typeset -f p10k >/dev/null 2>&1; then\n");
    functions.push_str("                    p10k reload 2>/dev/null\n");
    functions.push_str("                elif typeset -f _p9k_precmd >/dev/null 2>&1; then\n");
    functions.push_str("                    _p9k_precmd\n");
    functions.push_str("                fi\n");
    functions.push_str("            fi\n");
    functions.push_str("            ;;\n");
    functions.push_str("        *)\n");
    functions.push_str("            command sbenv \"$@\"\n");
    functions.push_str("            ;;\n");
    functions.push_str("    esac\n");
    functions.push_str("}\n");
    functions.push('\n');
    functions.push_str("# SyftBox environment aliases\n");
    functions.push_str("alias sba='sbenv activate'\n");
    functions.push_str("alias sbd='sbenv deactivate'\n");
    functions.push_str("alias sbi='sbenv info'\n");
    functions.push_str("alias sbl='sbenv list'\n");
    functions
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

    // Check if already installed
    if check_shell_functions_installed(&rc_file)? {
        println!(
            "{}",
            "✅ SyftBox shell functions are already installed!".green()
        );
        println!("The 'sbenv' command wrapper and aliases are ready to use.");
        println!();
        println!("If you haven't reloaded your shell config, run:");
        println!("  {}", format!("source {}", rc_file.display()).yellow());
        return Ok(());
    }

    // Show what will be added
    println!("The following will be added to your {} file:", shell_name);
    println!("{}", "─".repeat(50).dimmed());
    print!("{}", get_shell_functions().dimmed());
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

    existing_content.push_str(&get_shell_functions());

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
    println!("  {} - List environments (shortcut)", "sbl".cyan());

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Init { email, server_url }) => {
            init_environment(email.clone(), server_url.clone())?;
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
        Some(Commands::List) => {
            list_environments()?;
        }
        Some(Commands::Remove { path }) => {
            remove_environment(path.clone())?;
        }
        Some(Commands::InstallShell { manual }) => {
            if *manual {
                println!("# Add these functions to your shell configuration:");
                println!("# For ZSH: add to ~/.zshrc");
                println!("# For Bash: add to ~/.bashrc");
                print!("{}", get_shell_functions());
                println!();
                println!("After adding these functions, restart your shell or run:");
                println!("  source ~/.zshrc  # for ZSH");
                println!("  source ~/.bashrc # for Bash");
            } else {
                install_shell_functions()?;
            }
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
                println!("  {} - List all environments", "sbenv list".yellow());
                println!("  {} - Remove an environment", "sbenv remove".yellow());
                println!(
                    "  {} - Install shell functions for easier use",
                    "sbenv install-shell".yellow()
                );
                println!();
                println!("Use {} for more information", "sbenv --help".cyan());
            }
        }
    }

    Ok(())
}
