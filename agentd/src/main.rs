use clap::{Parser, Subcommand};
use libagent::{ResourceLimits, Sandbox, socket_server};

/// Command-line interface for the agent runtime.
#[derive(Parser)]
#[command(name = "agentd")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new sandbox and print its id
    CreateSandbox {
        #[arg(long)]
        ram: Option<u64>,
        #[arg(long)]
        cpu: Option<u64>,
    },
    /// Run a prompt using an agent in a sandbox
    Run {
        #[arg(long)]
        sandbox: u64,
        prompt: String,
    },
    /// Register a tool with the sandbox
    RegisterTool {
        #[arg(long)]
        sandbox: u64,
        #[arg(long)]
        name: String,
    },
    /// Invoke a tool with JSON input
    InvokeTool {
        #[arg(long)]
        sandbox: u64,
        #[arg(long)]
        name: String,
        #[arg(long)]
        input: String,
    },
    /// List all active sandboxes
    List,
    /// Get status of an agent
    Status {
        #[arg(long)]
        agent: u64,
    },
    /// Start Unix socket API server
    Socket {
        #[arg(long, default_value = "/tmp/agentd.sock")]
        path: String,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    
    match cli.cmd {
        Commands::CreateSandbox { ram, cpu } => {
            let limits = ResourceLimits {
                ram_bytes: ram,
                cpu_millis: cpu,
            };
            match Sandbox::new(limits) {
                Ok(sb) => println!("created sandbox {}", sb.id()),
                Err(e) => eprintln!("error: {}", e),
            }
        }
        Commands::Run { sandbox: _, prompt: _ } => {
            println!("run: use library API directly for now");
        }
        Commands::RegisterTool { sandbox: _, name } => {
            println!("registered tool {} - use library API", name);
        }
        Commands::InvokeTool { sandbox: _, name, input: _ } => {
            println!("invoked {} - use library API", name);
        }
        Commands::List => {
            println!("list: use persistence layer or library API");
        }
        Commands::Status { agent: _ } => {
            println!("status: placeholder");
        }
        Commands::Socket { path } => {
            if let Err(e) = socket_server::run_server(&path) {
                eprintln!("socket server error: {}", e);
            }
        }
    }
    Ok(())
}
