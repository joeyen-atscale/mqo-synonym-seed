use clap::{Parser, Subcommand, ValueEnum};
use mqo_synonym_seed::{
    apply_synonyms, generate_synonyms, process_message, Column, DescribeModel, GenerateOutput,
    SynonymCandidate,
};
use std::collections::HashMap;
use std::io::{BufRead, Write as IoWrite};
use std::path::PathBuf;

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Table,
}

#[derive(Debug, Parser)]
#[command(name = "mqo-synonym-seed", about = "Generate NL synonym sets for measures to improve AI retrieval")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate synonym candidates for measures/dimensions
    Generate {
        /// Path to describe_model JSON file
        #[arg(short, long)]
        model: PathBuf,

        /// Output format
        #[arg(short, long, value_enum, default_value = "json")]
        format: OutputFormat,

        /// Generate for all elements, even those with existing descriptions
        #[arg(long)]
        all: bool,

        /// Use Claude API for richer synonyms (falls back to rule path if API key absent)
        #[arg(long)]
        planner_brain: bool,
    },

    /// Apply approved synonyms into model JSON description fields
    Apply {
        /// Path to describe_model JSON file
        #[arg(short, long)]
        model: PathBuf,

        /// Path to approved synonyms JSON: {"COLUMN_NAME": ["syn1", "syn2"]}
        #[arg(short, long)]
        approved: PathBuf,

        /// Output path (defaults to stdout)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Run as a JSON-RPC 2.0 stdio server
    Serve,
}

fn main() {
    sigpipe::reset();
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate {
            model,
            format,
            all,
            planner_brain,
        } => cmd_generate(model, format, all, planner_brain),
        Commands::Apply { model, approved, out } => cmd_apply(model, approved, out),
        Commands::Serve => cmd_serve(),
    }
}

fn load_model(path: &PathBuf) -> DescribeModel {
    let data = std::fs::read_to_string(path)
        .unwrap_or_else(|e| die(&format!("failed to read {}: {e}", path.display())));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| die(&format!("failed to parse model JSON: {e}")))
}

fn cmd_generate(model_path: PathBuf, format: OutputFormat, all: bool, planner_brain: bool) {
    // --planner-brain: check for API key; if absent, warn and fall back to rule path
    if planner_brain {
        match std::env::var("ANTHROPIC_API_KEY") {
            Ok(key) if !key.is_empty() => {
                eprintln!("warning: --planner-brain requested but LLM integration is not yet implemented; falling back to rule-based path");
            }
            _ => {
                eprintln!("warning: --planner-brain requested but ANTHROPIC_API_KEY is not set; falling back to rule-based path");
            }
        }
    }

    let model = load_model(&model_path);
    let only_empty = !all;
    let candidates = generate_synonyms(&model.columns, only_empty);

    match format {
        OutputFormat::Json => {
            let output = GenerateOutput { candidates };
            println!("{}", serde_json::to_string_pretty(&output).expect("serialization failed"));
        }
        OutputFormat::Table => {
            print_table(&candidates);
        }
    }
}

fn print_table(candidates: &[SynonymCandidate]) {
    let name_w = candidates.iter().map(|c| c.name.len()).max().unwrap_or(4).max(4);
    println!("{:<name_w$}  SYNONYMS", "NAME", name_w = name_w);
    println!("{}", "-".repeat(name_w + 2 + 60));
    for c in candidates {
        println!(
            "{:<name_w$}  {}",
            c.name,
            c.synonyms.join("; "),
            name_w = name_w
        );
    }
}

fn cmd_apply(model_path: PathBuf, approved_path: PathBuf, out: Option<PathBuf>) {
    let mut model = load_model(&model_path);

    let approved_data = std::fs::read_to_string(&approved_path)
        .unwrap_or_else(|e| die(&format!("failed to read {}: {e}", approved_path.display())));
    let approved: HashMap<String, Vec<String>> = serde_json::from_str(&approved_data)
        .unwrap_or_else(|e| die(&format!("failed to parse approved JSON: {e}")));

    apply_synonyms(&mut model.columns, &approved);

    let output = serde_json::to_string_pretty(&model).expect("serialization failed");
    match out {
        Some(path) => {
            std::fs::write(&path, output)
                .unwrap_or_else(|e| die(&format!("failed to write {}: {e}", path.display())));
        }
        None => println!("{output}"),
    }
}

fn cmd_serve() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        match line {
            Ok(l) if l.trim().is_empty() => continue,
            Ok(l) => {
                let response = match serde_json::from_str::<serde_json::Value>(&l) {
                    Ok(req) => process_message(&req),
                    Err(e) => {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": { "code": -32700, "message": format!("parse error: {e}") }
                        })
                        .to_string()
                    }
                };
                writeln!(out, "{response}").unwrap_or_else(|_| std::process::exit(0));
                out.flush().unwrap_or(());
            }
            Err(_) => break,
        }
    }
}

// Avoid unwrap in production code — use this helper instead
fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

// Suppress dead-code warnings for types used only through lib exports in this binary
#[allow(dead_code)]
fn _type_check(_: Column) {}
