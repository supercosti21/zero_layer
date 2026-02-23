use clap::CommandFactory;
use clap_complete::{Shell, generate};

use crate::error::ZlResult;

use super::{Cli, CompletionsArgs};

pub fn handle(args: CompletionsArgs) -> ZlResult<()> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(args.shell, &mut cmd, name, &mut std::io::stdout());
    Ok(())
}

/// Print usage instructions for installing shell completions
#[allow(dead_code)]
pub fn print_instructions(shell: Shell) {
    match shell {
        Shell::Bash => {
            println!("# Add to ~/.bashrc:");
            println!("eval \"$(zl completions bash)\"");
        }
        Shell::Zsh => {
            println!("# Add to ~/.zshrc:");
            println!("eval \"$(zl completions zsh)\"");
        }
        Shell::Fish => {
            println!("# Run once:");
            println!("zl completions fish > ~/.config/fish/completions/zl.fish");
        }
        _ => {
            println!("# Pipe the output to your shell's completion file:");
            println!("zl completions {:?} > /path/to/completions", shell);
        }
    }
}
