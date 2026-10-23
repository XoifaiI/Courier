//! Courier schema compiler.
//!
//! ```text
//! courier                  compile every folder of schemas here into itself
//! courier watch            the same, again whenever a schema changes, appears or goes
//! courier lsp              language server for the editor
//! courier vscode           install the editor extension into VS Code, Cursor and the like
//! courier [--codec] [--runtime <path>] <schema or folder> [<output>]
//! ```

mod lsp;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime};

use courier_compiler::codegen::{File, Mode, Options, Runtime};

const SKIPPED: [&str; 3] = ["node_modules", "Packages", "target"];

const RUNTIME: [(&str, &str); 4] = [
    ("Courier/init.luau", include_str!("../../src/init.luau")),
    ("Courier/Future.luau", include_str!("../../src/Future.luau")),
    ("Courier/Result.luau", include_str!("../../src/Result.luau")),
    ("Courier/Types.luau", include_str!("../../src/Types.luau")),
];

const FRONT: &str = "require(\"@self/Implementation\")";

#[cfg(vsix)]
const EXTENSION: Option<&[u8]> = Some(include_bytes!(env!("COURIER_VSIX")));
#[cfg(not(vsix))]
const EXTENSION: Option<&[u8]> = None;

const EDITORS: [(&str, &str); 4] =
    [("code", "VS Code"), ("code-insiders", "VS Code Insiders"), ("cursor", "Cursor"), ("windsurf", "Windsurf")];

const MAC_EDITORS: [(&str, &str); 4] = [
    ("code", "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"),
    ("code-insiders", "/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/bin/code"),
    ("cursor", "/Applications/Cursor.app/Contents/Resources/app/bin/cursor"),
    ("windsurf", "/Applications/Windsurf.app/Contents/Resources/app/bin/windsurf"),
];

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("usage: courier [watch | vscode] | courier [--codec] [--runtime <path>] <schema or folder> [<output>]")]
    Usage,
    #[error(
        "this courier was built without the editor extension; run `npm run package` in editors/vscode, then build again"
    )]
    NoExtension,
    #[error(
        "found no VS Code, Cursor or Windsurf command line; in VS Code, run \"Shell Command: Install 'code' command in PATH\" from the command palette, then try again"
    )]
    NoEditor,
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("{schema}:{problem}\n{excerpt}")]
    Schema { schema: String, problem: courier_compiler::Error, excerpt: String },
    #[error("{folder}: {problem}")]
    Compile { folder: String, problem: courier_compiler::Error },
    #[error("could not write {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error(
        "{path} is not one Courier wrote, so it was left alone; keep the schemas in a folder of their own, which becomes the module"
    )]
    NotOurs { path: PathBuf },
    #[error("{0} holds no .courier file")]
    NoSchemas(PathBuf),
    #[error("no .courier file here, so there is nothing to compile")]
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
    if paths.next().is_some() || (command.mode == Mode::Codec && command.output.is_none()) {
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
    if tidied.as_os_str().is_empty() { PathBuf::from(".") } else { tidied }
}

fn is_schema(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "courier")
}

fn put(path: &Path, text: &str) -> Result<(), Error> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    let failed = |source| Error::Write { path: path.to_path_buf(), source };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    std::fs::write(path, text).map_err(failed)
}

fn write(output: &Path, files: &[File], runtime: &Runtime) -> Result<(), Error> {
    let front = output.join("init.luau");
    if std::fs::read_to_string(&front).is_ok_and(|text| !text.contains(FRONT)) {
        return Err(Error::NotOurs { path: front });
    }
    for file in files {
        put(&output.join(file.path), &file.text)?;
    }
    if *runtime == Runtime::Bundled {
        for (path, text) in RUNTIME {
            put(&output.join(path), text)?;
        }
    }
    Ok(())
}

fn compile(command: &Command, schemas: &[PathBuf], output: &Path) -> Result<(), Error> {
    let mut sources = Vec::with_capacity(schemas.len());
    for path in schemas {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Read { path: path.clone(), source })?;
        let name = path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        sources.push((name, text));
    }
    let files: Vec<(&str, &str)> = sources.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
    let schema = courier_compiler::schema::parse_unit(&files).map_err(|problem| {
        let (path, excerpt) = match problem {
            courier_compiler::Error::Schema(span, _) => {
                let file = span.file as usize;
                (&schemas[file], excerpt(&sources[file].1, span.line, span.column))
            }
            _ => (&schemas[0], String::new()),
        };
        Error::Schema { schema: tidy(path).display().to_string(), problem, excerpt }
    })?;
    let runtime = match &command.runtime {
        Some(runtime) => Runtime::Expression(runtime_expression(runtime)),
        None => Runtime::Bundled,
    };
    let options = Options { mode: command.mode, runtime: runtime.clone() };
    let files = courier_compiler::compile_schema(&schema, &options)
        .map_err(|problem| Error::Compile { folder: tidy(output).display().to_string(), problem })?;
    if command.mode == Mode::Codec {
        return put(output, &files[0].text);
    }
    write(output, &files, &runtime)
}

fn schemas_in(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|path| is_schema(path)).collect();
    found.sort();
    found
}

fn folders(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut holds = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                folders(&path, found);
            }
        } else if is_schema(&path) {
            holds = true;
        }
    }
    if holds {
        found.push(folder.to_path_buf());
    }
}

fn units(command: &Command) -> Result<Vec<(Vec<PathBuf>, PathBuf)>, Error> {
    let Some(input) = &command.input else {
        let mut found = Vec::new();
        folders(Path::new("."), &mut found);
        found.sort();
        return Ok(found.into_iter().map(|folder| (schemas_in(&folder), tidy(&folder))).collect());
    };
    let (schemas, folder) = if input.is_dir() {
        (schemas_in(input), input.clone())
    } else {
        (vec![input.clone()], input.parent().map_or_else(PathBuf::new, Path::to_path_buf))
    };
    if schemas.is_empty() {
        return Err(Error::NoSchemas(input.clone()));
    }
    let output = command.output.clone().unwrap_or_else(|| tidy(&folder));
    Ok(vec![(schemas, output)])
}

fn names(schemas: &[PathBuf]) -> String {
    let names: Vec<String> =
        schemas.iter().filter_map(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned()).collect();
    names.join(", ")
}

fn report(command: &Command, schemas: &[PathBuf], output: &Path) -> bool {
    match compile(command, schemas, output) {
        Ok(()) => {
            println!("compiled {} into {}", names(schemas), output.display());
            true
        }
        Err(error) => {
            eprintln!("{error}");
            false
        }
    }
}

type Snapshot = Vec<(PathBuf, Option<SystemTime>)>;

fn snapshot(schemas: &[PathBuf]) -> Snapshot {
    schemas
        .iter()
        .map(|path| (path.clone(), std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()))
        .collect()
}

fn watch(command: &Command) -> Result<ExitCode, Error> {
    let mut seen: BTreeMap<PathBuf, (PathBuf, Snapshot)> = BTreeMap::new();
    let mut scanned = Instant::now() - Duration::from_secs(60);
    loop {
        if scanned.elapsed() >= Duration::from_secs(2) {
            scanned = Instant::now();
            for (schemas, output) in units(command)? {
                let folder = schemas[0].parent().map_or_else(PathBuf::new, Path::to_path_buf);
                seen.entry(folder).or_insert((output, Vec::new()));
            }
        }
        for (folder, (output, last)) in &mut seen {
            let schemas = match &command.input {
                Some(input) if !input.is_dir() => vec![input.clone()],
                _ => schemas_in(folder),
            };
            let now = snapshot(&schemas);
            if now == *last {
                continue;
            }
            *last = now;
            if schemas.is_empty() {
                println!("{} holds no schema now; what it compiled to is left as it is", folder.display());
            } else {
                report(command, &schemas, output);
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn editor(name: &str) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["cmd", "exe"] } else { &[""] };
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path).find_map(|folder| {
            names
                .iter()
                .map(|extension| folder.join(name).with_extension(extension))
                .find(|candidate| candidate.is_file())
        })
    });
    if found.is_some() || !cfg!(target_os = "macos") {
        return found;
    }
    MAC_EDITORS
        .iter()
        .find(|(command, _)| *command == name)
        .map(|(_, path)| PathBuf::from(path))
        .filter(|path| path.is_file())
}

fn install_extension() -> Result<ExitCode, Error> {
    let extension = EXTENSION.ok_or(Error::NoExtension)?;
    let editors: Vec<(PathBuf, &str)> =
        EDITORS.iter().filter_map(|(name, label)| editor(name).map(|path| (path, *label))).collect();
    if editors.is_empty() {
        return Err(Error::NoEditor);
    }
    let vsix = std::env::temp_dir().join(format!("courier-{}.vsix", env!("CARGO_PKG_VERSION")));
    std::fs::write(&vsix, extension).map_err(|source| Error::Write { path: vsix.clone(), source })?;
    let mut failed = false;
    for (path, label) in editors {
        let output = std::process::Command::new(&path).arg("--install-extension").arg(&vsix).arg("--force").output();
        match output {
            Ok(output) if output.status.success() => println!("installed the Courier extension in {label}"),
            Ok(output) => {
                failed = true;
                eprintln!("{label} could not install it: {}", String::from_utf8_lossy(&output.stderr).trim());
            }
            Err(problem) => {
                failed = true;
                eprintln!("could not run {}: {problem}", path.display());
            }
        }
    }
    let _ = std::fs::remove_file(&vsix);
    Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn run() -> Result<ExitCode, Error> {
    if std::env::args().nth(1).as_deref() == Some("vscode") {
        return if std::env::args().len() == 2 { install_extension() } else { Err(Error::Usage) };
    }
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
        return watch(&command);
    }
    let units = units(&command)?;
    if units.is_empty() {
        return Err(Error::NothingToCompile);
    }
    let mut failed = false;
    for (schemas, output) in &units {
        failed |= !report(&command, schemas, output);
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
