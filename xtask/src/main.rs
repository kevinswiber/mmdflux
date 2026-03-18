mod architecture;

use anyhow::Result;

fn main() {
    if let Err(err) = try_main() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    match command {
        "architecture" => run_architecture_command(&args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => anyhow::bail!("unknown xtask subcommand `{other}`"),
    }
}

fn run_architecture_command(args: &[String]) -> Result<()> {
    if args.iter().skip(1).any(|arg| is_help_arg(arg)) {
        print_architecture_help();
        return Ok(());
    }

    let options = architecture::parse_architecture_args(args.iter().map(String::as_str))?;
    architecture::run(options)
}

fn is_help_arg(arg: &str) -> bool {
    matches!(arg, "help" | "--help" | "-h")
}

fn print_help() {
    eprintln!(
        "\
cargo xtask <command>

Commands:
    architecture    Run the repo architecture suite

Run `cargo xtask architecture --help` for suite and flag details."
    );
}

fn print_architecture_help() {
    eprintln!("{}", architecture::help_text());
}
