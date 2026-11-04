use std::io::{stdin, stdout, BufReader};
use std::path::PathBuf;
use vault_mcp::{rpc, Config, Server};

const HELP: &str = "vault-mcp: read-only MCP server (stdio) over archived Claude Code sessions

USAGE:
    vault-mcp [--vault DIR] [--archive DIR] [--max-response-chars N]

OPTIONS:
    --vault DIR              Obsidian vault root   (env VAULT_MCP_VAULT, default ~/Downloads/obsidian-vault)
    --archive DIR            raw JSONL archive dir (env VAULT_MCP_ARCHIVE, default D:/Archive/claude-transcripts
                             on Windows, ~/Archive/claude-transcripts elsewhere)
    --max-response-chars N   hard cap per tool response (default 12000)
    -h, --help, -V, --version
";

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn parse_args() -> Result<Config, String> {
    let mut vault = std::env::var_os("VAULT_MCP_VAULT").map(PathBuf::from);
    let mut archive = std::env::var_os("VAULT_MCP_ARCHIVE").map(PathBuf::from);
    let mut max = 12000usize;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("vault-mcp {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--vault" => vault = Some(PathBuf::from(val("--vault")?)),
            "--archive" => archive = Some(PathBuf::from(val("--archive")?)),
            "--max-response-chars" => {
                max = val("--max-response-chars")?
                    .parse()
                    .map_err(|_| "--max-response-chars must be a number".to_string())?
            }
            other => return Err(format!("unknown argument `{other}` (try --help)")),
        }
    }
    let vault = vault.unwrap_or_else(|| home().join("Downloads").join("obsidian-vault"));
    let archive = archive.unwrap_or_else(|| {
        if cfg!(windows) {
            PathBuf::from("D:/Archive/claude-transcripts")
        } else {
            home().join("Archive").join("claude-transcripts")
        }
    });
    Ok(Config {
        vault,
        archive,
        max_response_chars: max.clamp(2000, 100_000),
    })
}

fn main() {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vault-mcp: {e}");
            std::process::exit(2);
        }
    };
    let mut server = Server::new(cfg);
    if let Err(e) = rpc::serve(&mut server, BufReader::new(stdin().lock()), stdout().lock()) {
        eprintln!("vault-mcp: io error: {e}");
        std::process::exit(1);
    }
}
