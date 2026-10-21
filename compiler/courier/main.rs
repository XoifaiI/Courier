//! Courier schema compiler.
//!
//! ```text
//! courier                  compile every schema here that sets an output
//! courier watch            the same, and again on every save
//! courier lsp              language server for the editor
//! courier [--codec] [--runtime <path>] [--watch] <schema> [<output>]
//! ```

mod lsp;
mod rbxmx;

use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime};

use courier_compiler::codegen::{File, Mode, Options};

const DEFAULT_RUNTIME: &str = "ReplicatedStorage.Courier";

const SKIPPED: [&str; 3] = ["node_modules", "Packages", "target"];

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("usage: courier [watch] | courier [--codec] [--runtime <path>] [--watch] <schema.courier> [<output>]")]
    Usage,
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("{schema}:{problem}\n{excerpt}")]
    Schema { schema: String, problem: courier_compiler::Error, excerpt: String },
    #[error("{schema}: {problem}")]
    Compile { schema: String, problem: courier_compiler::Error },
    #[error("{schema}: no output; pass one after the schema or add `option output = \"path\"` to it")]
    NoOutput { schema: String },
    #[error("could not write {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("no .courier file here sets `option output`, so there is nothing to compile")]
    NothingToCompile,
}

struct Command {
    mode: Mode,
    runtime: Option<String>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    watch: bool,
}

fn command() -> Result<Command, Error> {
    let mut command = Command { mode: Mode::Api, runtime: None, input: None, output: None, watch: false };
    let mut paths = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--codec" => command.mode = Mode::Codec,
            "--runtime" => command.runtime = Some(arguments.next().ok_or(Error::Usage)?),
            "--watch" => command.watch = true,
            _ if argument.starts_with("--") => return Err(Error::Usage),
            _ => paths.push(PathBuf::from(argument)),
        }
    }
    if paths.len() == 1 && paths[0] == Path::new("watch") {
        command.watch = true;
        paths.clear();
    }
    let mut paths = paths.into_iter();
    command.input = paths.next();
    command.output = paths.next();
    if paths.next().is_some() {
        return Err(Error::Usage);
    }
    Ok(command)
}

fn runtime_expression(runtime: &str) -> String {
    if runtime.contains('(') {
        return runtime.to_string();
    }
    let mut steps = runtime.split('.');
    let mut expression = format!("game:GetService(\"{}\")", steps.next().unwrap_or("ReplicatedStorage"));
    for step in steps {
        expression.push_str(&format!(":WaitForChild(\"{step}\")"));
    }
    expression
}

fn excerpt(source: &str, line: u32, column: u32) -> String {
    let text = source.lines().nth(line as usize - 1).unwrap_or("");
    let number = line.to_string();
    let pad = " ".repeat(number.len());
    let lead: String = text.chars().take(column as usize - 1).map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
    format!("{number} | {text}\n{pad} | {lead}^")
}

fn tidy(path: &Path) -> PathBuf {
    let mut tidied = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if matches!(tidied.components().next_back(), Some(Component::Normal(_))) => {
                tidied.pop();
            }
            other => tidied.push(other),
        }
    }
    tidied
}

fn write(output: &Path, files: &[File]) -> Result<(), Error> {
    for file in files {
        let path = if file.path.is_empty() { output.to_path_buf() } else { output.join(file.path) };
        let failed = |source| Error::Write { path: path.clone(), source };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(failed)?;
        }
        std::fs::write(&path, &file.text).map_err(failed)?;
    }
    Ok(())
}

fn compile(command: &Command, input: &Path, quiet_without_output: bool) -> Result<Option<PathBuf>, Error> {
    let source = std::fs::read_to_string(input).map_err(|source| Error::Read { path: input.to_path_buf(), source })?;
    let name = tidy(input).display().to_string();
    let parsed = courier_compiler::schema::parse(&source);
    let schema = parsed.map_err(|problem| {
        let excerpt = match problem {
            courier_compiler::Error::Schema(span, _) => excerpt(&source, span.line, span.column),
            _ => String::new(),
        };
        Error::Schema { schema: name.clone(), problem, excerpt }
    })?;
    let folder = input.parent().unwrap_or(Path::new(""));
    let output = match (&command.output, &schema.output) {
        (Some(output), _) => output.clone(),
        (None, Some(output)) => tidy(&folder.join(output.as_ref())),
        (None, None) if quiet_without_output => return Ok(None),
        (None, None) => return Err(Error::NoOutput { schema: name }),
    };
    let runtime = command.runtime.as_deref().or(schema.runtime.as_deref()).unwrap_or(DEFAULT_RUNTIME);
    let options = Options { mode: command.mode, runtime: runtime_expression(runtime) };
    let files = courier_compiler::compile_schema(&schema, &options)
        .map_err(|problem| Error::Compile { schema: name.clone(), problem })?;
    let model = output.extension().is_some_and(|extension| extension == "rbxmx");
    if model && command.mode == Mode::Api {
        let module = output.file_stem().map_or("Net".into(), |stem| stem.to_string_lossy().into_owned());
        let text = rbxmx::model(&module, &files);
        std::fs::write(&output, text).map_err(|source| Error::Write { path: output.clone(), source })?;
    } else {
        write(&output, &files)?;
    }
    Ok(Some(output))
}

fn schemas(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                schemas(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "courier") {
            found.push(path);
        }
    }
}

fn targets(command: &Command) -> Vec<PathBuf> {
    match &command.input {
        Some(input) => vec![input.clone()],
        None => {
            let mut found = Vec::new();
            schemas(Path::new("."), &mut found);
            found.sort();
            found
        }
    }
}

fn report(command: &Command, input: &Path) -> bool {
    match compile(command, input, command.input.is_none()) {
        Ok(Some(output)) => {
            println!("compiled {} into {}", tidy(input).display(), output.display());
            true
        }
        Ok(None) => false,
        Err(error) => {
            eprintln!("{error}");
            true
        }
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

fn watch(command: &Command) -> ! {
    let mut seen: Vec<(PathBuf, Option<SystemTime>)> = Vec::new();
    let mut scanned = Instant::now() - Duration::from_secs(60);
    loop {
        if scanned.elapsed() >= Duration::from_secs(2) {
            scanned = Instant::now();
            for path in targets(command) {
                if !seen.iter().any(|(known, _)| *known == path) {
                    seen.push((path, None));
                }
            }
        }
        for (path, last) in &mut seen {
            let now = modified(path);
            if now != *last {
                *last = now;
                report(command, path);
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn run() -> Result<ExitCode, Error> {
    if std::env::args().nth(1).as_deref() == Some("lsp") {
        return Ok(match lsp::serve() {
            Ok(()) => ExitCode::SUCCESS,
            Err(problem) => {
                eprintln!("courier lsp: {problem}");
                ExitCode::FAILURE
            }
        });
    }
    let command = command()?;
    if command.output.is_some() && command.input.is_none() {
        return Err(Error::Usage);
    }
    if command.watch {
        println!("watching for changes; stop with Ctrl+C");
        watch(&command);
    }
    if let Some(input) = &command.input {
        let output = compile(&command, input, false)?;
        if let Some(output) = output {
            println!("compiled {} into {}", tidy(input).display(), output.display());
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut any = false;
    let mut failed = false;
    for path in targets(&command) {
        match compile(&command, &path, true) {
            Ok(Some(output)) => {
                println!("compiled {} into {}", tidy(&path).display(), output.display());
                any = true;
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("{error}");
                any = true;
                failed = true;
            }
        }
    }
    if !any {
        return Err(Error::NothingToCompile);
    }
    Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
