use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};

use toml_edit::{Array, DocumentMut, Item, RawString, Value};

const USAGE: &str = "\
Usage: scaffold <topic> [program ...] [options]

Creates a topic crate at the repository root, or adds programs to one that is
already there. The manifest is written by hand rather than by cargo new, so a
topic named after a Rust keyword works too: cargo new rejects struct, enum,
trait, async and unsafe, while a hand-written manifest builds them.

Options:
  -d, --dir <name>   a program spanning several files, as src/bin/<name>/main.rs
  -l, --lib          add src/lib.rs, for code shared by the topic's programs
  -n, --dry-run      print the plan and write nothing
  -t, --topics       print the known topic names, one per line, and exit
  -h, --help         this text

Layout it writes, which is the one README.md documents:

  <topic>/Cargo.toml            three lines, inheriting the workspace edition
  <topic>/src/lib.rs            optional, shared by the topic's programs
  <topic>/src/bin/<name>.rs     one program, needs a main
  <topic>/src/bin/<name>/       one program spanning several files
      main.rs                   the crate root of that program
                                siblings go beside it and need a mod declaration

A new topic gets the programs named on the command line. Name none and it gets
one program named after the topic, unless --lib is passed, which then creates
the library alone. Cargo finds every program through the src/bin convention, so
nothing is ever declared in a topic manifest.

Every binary target in the workspace lands in target/debug/, so binary names
must be unique across topics. Cargo only warns about a collision and lets one
binary overwrite the other; this tool refuses it instead.

The members array of the root manifest is read and rewritten through a TOML
parser, so its formatting, comments and quoting style do not affect the edit.
The new entry is spliced in at its sorted position; entries already there keep
their order, their comments and their formatting, and a byte order mark, CRLF
line endings and a missing final newline all survive. A topic already listed
leaves the manifest untouched.

One run at a time: the tool locks .scaffold.lock in the repository root for the
whole read and rewrite, so a second run waits for the first to finish and a lock
file left behind by a killed run is harmless. If a write fails, the files
written before it are removed again and the repository is left as it was.

Examples:
  scaffold slice borrowed_view chunks
  scaffold lifetime elision --dir bounds --lib
  scaffold borrow --dry-run
";

const PROGRAM_STUB: &str = "fn main() {}\n";
const NAME_LIMIT: usize = 64;
const LOCK_FILE: &str = ".scaffold.lock";
const LOCK_ATTEMPTS: u32 = 64;
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
const LONG_OPTIONS: &[&str] = &["--dir", "--lib", "--dry-run", "--topics", "--help"];
const SHORT_OPTIONS: &str = "dlnth";
const DEFAULT_INDENT: &str = "    ";

#[derive(Debug)]
struct Error {
    message: String,
    code: u8,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn failure(message: impl Into<String>) -> Error {
    Error {
        message: message.into(),
        code: 1,
    }
}

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(failure(message))
}

fn cannot(action: &str, path: &Path, cause: impl fmt::Display) -> Error {
    let cause = cause.to_string();
    failure(format!(
        "cannot {action} {}: {}",
        path.display(),
        cause.trim_end()
    ))
}

fn warn(message: impl fmt::Display) {
    eprintln!("scaffold: warning: {message}");
}

fn emit(text: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|cause| match cause.kind() {
            ErrorKind::BrokenPipe => Error {
                message: String::new(),
                code: 1,
            },
            _ => failure(format!("cannot write to stdout: {cause}")),
        })
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if !error.message.is_empty() {
                eprintln!("scaffold: {error}");
            }
            ExitCode::from(error.code)
        }
    }
}

struct Args {
    topic: String,
    flat: Vec<String>,
    dirs: Vec<String>,
    lib: bool,
    dry_run: bool,
}

enum Request {
    Handled,
    Topics,
    Create(Args),
}

fn text_argument(raw: OsString) -> Result<String> {
    raw.into_string().map_err(|raw| {
        failure(format!(
            "argument {} is not valid UTF-8",
            raw.to_string_lossy()
        ))
    })
}

fn option_hint(arg: &str) -> Option<String> {
    let (name, value) = arg
        .split_once('=')
        .map_or((arg, None), |(name, value)| (name, Some(value)));
    if name.starts_with("--") {
        return match value {
            Some(value) if name == "--dir" => Some(format!(
                "the name goes in the next argument: --dir {}",
                if value.is_empty() { "<name>" } else { value }
            )),
            Some(_) if LONG_OPTIONS.contains(&name) => Some(format!("{name} takes no value")),
            _ => nearest(name, LONG_OPTIONS.iter().copied())
                .map(|near| format!("did you mean {near}?")),
        };
    }
    if arg.len() > 2 && arg[1..].chars().all(|c| SHORT_OPTIONS.contains(c)) {
        let separated: Vec<String> = arg[1..].chars().map(|c| format!("-{c}")).collect();
        return Some(format!(
            "short options are given one at a time: {}",
            separated.join(" ")
        ));
    }
    nearest(arg, LONG_OPTIONS.iter().copied()).map(|near| format!("did you mean {near}?"))
}

fn unknown_option(arg: &str) -> Error {
    let hint = option_hint(arg).unwrap_or_else(|| "see --help".to_owned());
    failure(format!("unknown option {arg}, {hint}"))
}

fn parse_args() -> Result<Request> {
    let mut topic = None;
    let mut flat = Vec::new();
    let mut dirs = Vec::new();
    let mut lib = false;
    let mut dry_run = false;

    let mut rest = env::args_os().skip(1);
    while let Some(raw) = rest.next() {
        let arg = text_argument(raw)?;
        match arg.as_str() {
            "-h" | "--help" => {
                emit(USAGE)?;
                return Ok(Request::Handled);
            }
            "-t" | "--topics" => return Ok(Request::Topics),
            "-n" | "--dry-run" => dry_run = true,
            "-l" | "--lib" => lib = true,
            "-d" | "--dir" => match rest.next().map(text_argument).transpose()? {
                Some(name) if !name.starts_with('-') => dirs.push(name),
                Some(next) => return fail(format!("{arg} needs a name, found {next}")),
                None => return fail(format!("{arg} needs a name")),
            },
            _ if arg.starts_with('-') => return Err(unknown_option(&arg)),
            _ if topic.is_none() => topic = Some(arg),
            _ => flat.push(arg),
        }
    }

    let Some(topic) = topic else {
        eprint!("{USAGE}");
        return Err(Error {
            message: String::new(),
            code: 2,
        });
    };
    Ok(Request::Create(Args {
        topic,
        flat,
        dirs,
        lib,
        dry_run,
    }))
}

fn check_name(kind: &str, name: &str) -> Result<()> {
    let mut chars = name.chars();
    let shaped = matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !shaped {
        return fail(format!("{kind} '{name}' must match ^[a-z][a-z0-9_]*$"));
    }
    if name.len() > NAME_LIMIT {
        return fail(format!(
            "{kind} '{name}' is longer than {NAME_LIMIT} characters"
        ));
    }
    Ok(())
}

struct Lock {
    path: PathBuf,
    _file: File,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn still_at(path: &Path, file: &File) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(path), file.metadata()) {
        (Ok(named), Ok(held)) => named.dev() == held.dev() && named.ino() == held.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn still_at(path: &Path, _file: &File) -> bool {
    fs::metadata(path).is_ok()
}

fn take_lock(root: &Path) -> Result<Lock> {
    let path = root.join(LOCK_FILE);
    let mut waited = false;
    for _ in 0..LOCK_ATTEMPTS {
        let (file, created) = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => (file, true),
            Err(cause) if cause.kind() == ErrorKind::AlreadyExists => match File::open(&path) {
                Ok(file) => (file, false),
                Err(cause) if cause.kind() == ErrorKind::NotFound => continue,
                Err(cause) => return Err(cannot("open", &path, cause)),
            },
            Err(cause) => return Err(cannot("create", &path, cause)),
        };
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                if !waited {
                    eprintln!("scaffold: waiting for another scaffold run to finish");
                    waited = true;
                }
                file.lock().map_err(|cause| cannot("lock", &path, cause))?;
            }
            Err(TryLockError::Error(_)) if created => {}
            Err(TryLockError::Error(_)) => {
                return fail(format!(
                    "another scaffold run holds {}, delete it if no scaffold is running",
                    path.display()
                ));
            }
        }
        if !still_at(&path, &file) {
            continue;
        }
        if created {
            let _ = writeln!(&file, "{}", process::id());
        }
        return Ok(Lock { path, _file: file });
    }
    fail(format!(
        "cannot take {} after {LOCK_ATTEMPTS} attempts",
        path.display()
    ))
}

struct Style {
    bom: bool,
    crlf: bool,
    final_newline: bool,
}

fn load(path: &Path) -> Result<(DocumentMut, Style)> {
    let bytes = fs::read(path).map_err(|cause| cannot("read", path, cause))?;
    let bom = bytes.starts_with(&BOM);
    let body = if bom { &bytes[BOM.len()..] } else { &bytes[..] };
    let text = std::str::from_utf8(body).map_err(|cause| cannot("read", path, cause))?;
    let style = Style {
        bom,
        crlf: text.contains("\r\n") && !text.replace("\r\n", "").contains('\n'),
        final_newline: text.ends_with('\n'),
    };
    let doc = text
        .parse::<DocumentMut>()
        .map_err(|cause| cannot("parse", path, cause))?;
    Ok((doc, style))
}

fn read_document(path: &Path) -> Result<DocumentMut> {
    load(path).map(|(doc, _)| doc)
}

fn styled(text: &str, style: &Style) -> Vec<u8> {
    let mut out = text.to_string();
    if style.final_newline {
        if !out.ends_with('\n') {
            out.push('\n');
        }
    } else {
        while out.ends_with('\n') || out.ends_with('\r') {
            out.pop();
        }
    }
    if style.crlf {
        out = out.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    let mut bytes = Vec::with_capacity(out.len() + BOM.len());
    if style.bom {
        bytes.extend_from_slice(&BOM);
    }
    bytes.extend_from_slice(out.as_bytes());
    bytes
}

fn workspace_members(doc: &DocumentMut) -> Option<&Array> {
    doc.get("workspace")?.get("members")?.as_array()
}

fn is_workspace_root(doc: &DocumentMut) -> bool {
    doc.get("workspace")
        .and_then(Item::as_table_like)
        .is_some_and(|workspace| !workspace.is_empty())
}

fn find_root() -> Result<PathBuf> {
    let cwd = env::current_dir()
        .map_err(|cause| failure(format!("cannot read the working directory: {cause}")))?;
    let mut notes = Vec::new();
    for dir in cwd.ancestors() {
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        match read_document(&manifest) {
            Ok(doc) if is_workspace_root(&doc) => return Ok(dir.to_path_buf()),
            Ok(doc) if doc.get("workspace").is_some() => notes.push(format!(
                "{} has an empty [workspace] table",
                manifest.display()
            )),
            Ok(_) => notes.push(format!("{} has no [workspace] table", manifest.display())),
            Err(error) => notes.push(error.message),
        }
    }
    let mut message = format!(
        "no workspace manifest above {}, run this inside the repository",
        cwd.display()
    );
    for note in notes {
        message.push_str("\n  note: ");
        message.push_str(&note);
    }
    fail(message)
}

fn as_topic(entry: &str) -> &str {
    entry
        .trim_start_matches("./")
        .trim_end_matches('/')
        .trim_end_matches("/*")
}

fn member_names(doc: &DocumentMut, path: &Path) -> Result<Vec<String>> {
    let array = workspace_members(doc)
        .ok_or_else(|| failure(format!("{} has no workspace.members array", path.display())))?;
    array
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                failure(format!(
                    "{} has a workspace.members entry that is not a string",
                    path.display()
                ))
            })
        })
        .collect()
}

fn prefix_text(value: &Value) -> &str {
    value
        .decor()
        .prefix()
        .and_then(RawString::as_str)
        .unwrap_or("")
}

fn suffix_text(value: &Value) -> &str {
    value
        .decor()
        .suffix()
        .and_then(RawString::as_str)
        .unwrap_or("")
}

fn indent_after_newline(text: &str) -> Option<&str> {
    text.rsplit_once('\n').map(|(_, indent)| indent)
}

fn array_is_multiline(array: &Array) -> bool {
    array
        .iter()
        .any(|value| prefix_text(value).contains('\n') || suffix_text(value).contains('\n'))
        || array.trailing().as_str().is_some_and(|s| s.contains('\n'))
}

fn splice_line(array: &mut Array, index: usize, topic: &str) {
    let previous = index.checked_sub(1).and_then(|i| array.get(i));
    let indent = previous
        .and_then(|value| indent_after_newline(prefix_text(value)))
        .or_else(|| {
            array
                .get(index)
                .and_then(|value| indent_after_newline(prefix_text(value)))
        })
        .or_else(|| {
            array
                .iter()
                .find_map(|value| indent_after_newline(prefix_text(value)))
        })
        .unwrap_or(DEFAULT_INDENT)
        .to_owned();
    let following = array
        .get(index)
        .map_or_else(|| array.trailing().as_str().unwrap_or(""), prefix_text);
    let between = format!("{}{following}", previous.map_or("", suffix_text));

    let split = between.find('\n').map(|at| between.split_at(at));
    let prefix = match split {
        Some((head, _)) if !head.trim().is_empty() => format!("{head}\n{indent}"),
        _ => format!("\n{indent}"),
    };
    if let Some((_, tail)) = split {
        let tail = tail.to_owned();
        if let Some(i) = index.checked_sub(1)
            && let Some(value) = array.get_mut(i)
        {
            value.decor_mut().set_suffix("");
        }
        match array.get_mut(index) {
            Some(next) => next.decor_mut().set_prefix(tail),
            None => array.set_trailing(tail),
        }
    }

    array.insert(index, topic);
    if let Some(value) = array.get_mut(index) {
        let decor = value.decor_mut();
        decor.set_prefix(prefix);
        decor.set_suffix("");
    }
}

fn insert_member(doc: &mut DocumentMut, topic: &str) -> Result<()> {
    let array = doc
        .get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("members"))
        .and_then(Item::as_array_mut)
        .ok_or_else(|| failure("the root manifest has no workspace.members array"))?;

    let index = array
        .iter()
        .position(|value| value.as_str().is_some_and(|name| as_topic(name) > topic))
        .unwrap_or(array.len());

    if array_is_multiline(array) {
        splice_line(array, index, topic);
    } else {
        array.insert(index, topic);
        if let Some(value) = array.get_mut(index) {
            let decor = value.decor_mut();
            decor.set_prefix(if index == 0 { "" } else { " " });
            decor.set_suffix("");
        }
        if index == 0
            && let Some(value) = array.get_mut(1)
        {
            value.decor_mut().set_prefix(" ");
        }
    }
    Ok(())
}

fn topic_names(root: &Path) -> Result<Vec<String>> {
    let mut names = BTreeSet::new();
    let entries = fs::read_dir(root).map_err(|cause| cannot("read", root, cause))?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if dir.join("Cargo.toml").is_file()
            && let Some(name) = dir.file_name().and_then(|name| name.to_str())
        {
            names.insert(name.to_string());
        }
    }
    names.extend(roadmap_names(root));
    Ok(names.into_iter().collect())
}

fn taken_bins(root: &Path) -> Result<BTreeSet<String>> {
    let mut bins = BTreeSet::new();
    let entries = fs::read_dir(root).map_err(|cause| cannot("read", root, cause))?;

    for entry in entries {
        let dir = entry.map_err(|cause| cannot("read", root, cause))?.path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let doc = match read_document(&manifest) {
            Ok(doc) => doc,
            Err(error) => {
                warn(format!(
                    "the binary names in {} are not checked\n  note: {error}",
                    relative(&manifest, root)
                ));
                continue;
            }
        };
        let package = doc
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(Item::as_str);
        if let Some(package) = package
            && dir.join("src").join("main.rs").is_file()
        {
            bins.insert(package.to_string());
        }
        if let Some(tables) = doc.get("bin").and_then(Item::as_array_of_tables) {
            for table in tables {
                if let Some(name) = table.get("name").and_then(Item::as_str) {
                    bins.insert(name.to_string());
                }
            }
        }

        let autobins = doc
            .get("package")
            .and_then(|package| package.get("autobins"))
            .and_then(Item::as_bool)
            .unwrap_or(true);
        if !autobins {
            continue;
        }
        let Ok(programs) = fs::read_dir(dir.join("src").join("bin")) else {
            continue;
        };
        for program in programs.flatten() {
            let path = program.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    bins.insert(stem.to_string());
                }
            } else if path.is_dir()
                && path.join("main.rs").is_file()
                && let Some(name) = path.file_name().and_then(|name| name.to_str())
            {
                bins.insert(name.to_string());
            }
        }
    }
    Ok(bins)
}

fn roadmap_names(root: &Path) -> Vec<String> {
    let path = root.join(".project.toml");
    if !path.is_file() {
        return Vec::new();
    }
    let doc = match read_document(&path) {
        Ok(doc) => doc,
        Err(error) => {
            warn(format!("skipping the roadmap check\n  note: {error}"));
            return Vec::new();
        }
    };
    let Some(topics) = doc
        .get("taxonomy")
        .and_then(|taxonomy| taxonomy.get("topics"))
    else {
        return Vec::new();
    };
    if let Some(array) = topics.as_array() {
        return array
            .iter()
            .filter_map(Value::as_inline_table)
            .filter_map(|entry| entry.get("name"))
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    if let Some(tables) = topics.as_array_of_tables() {
        return tables
            .iter()
            .filter_map(|entry| entry.get("name"))
            .filter_map(Item::as_str)
            .map(str::to_owned)
            .collect();
    }
    Vec::new()
}

fn distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];

    for (i, a) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, b) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(a != b);
            current[j + 1] = (previous[j + 1] + 1).min(current[j] + 1).min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn nearest<'a>(word: &str, names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names
        .into_iter()
        .map(|name| (distance(word, name), name))
        .filter(|(gap, name)| *gap <= 2 && gap * 3 <= word.len().max(name.len()))
        .min_by_key(|(gap, _)| *gap)
        .map(|(_, name)| name)
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn reject_symlink(path: &Path, root: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fail(format!(
            "{} is a symlink, which would write outside the repository",
            relative(path, root)
        )),
        _ => Ok(()),
    }
}

fn reject_occupied(path: &Path, root: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_dir() => fail(format!(
            "{} exists and is not a directory",
            relative(path, root)
        )),
        _ => Ok(()),
    }
}

#[derive(Default)]
struct Created(Vec<PathBuf>);

impl Created {
    fn file(&mut self, path: &Path, body: &str) -> Result<()> {
        let missing: Vec<&Path> = path
            .ancestors()
            .skip(1)
            .take_while(|dir| fs::symlink_metadata(dir).is_err())
            .collect();
        for dir in missing.into_iter().rev() {
            match fs::create_dir(dir) {
                Ok(()) => self.0.push(dir.to_path_buf()),
                Err(cause) if cause.kind() == ErrorKind::AlreadyExists => {}
                Err(cause) => return Err(cannot("create", dir, cause)),
            }
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|cause| cannot("write", path, cause))?;
        self.0.push(path.to_path_buf());
        file.write_all(body.as_bytes())
            .map_err(|cause| cannot("write", path, cause))
    }

    fn undo(self) -> Vec<PathBuf> {
        self.0
            .into_iter()
            .rev()
            .filter(|path| {
                let removed = if path.is_dir() {
                    fs::remove_dir(path)
                } else {
                    fs::remove_file(path)
                };
                removed.is_err()
            })
            .collect()
    }
}

#[cfg(unix)]
fn hard_linked(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() > 1
}

#[cfg(not(unix))]
fn hard_linked(_metadata: &fs::Metadata) -> bool {
    false
}

fn replace_file(path: &Path, body: &[u8]) -> Result<()> {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let metadata = fs::metadata(&path).map_err(|cause| cannot("read", &path, cause))?;
    if metadata.permissions().readonly() {
        return fail(format!("{} is read-only", path.display()));
    }
    if hard_linked(&metadata) {
        return fs::write(&path, body).map_err(|cause| cannot("write", &path, cause));
    }

    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Cargo.toml");

    for attempt in 0..64u32 {
        let temporary = directory.join(format!(".{name}.scaffold.{}.{attempt}", process::id()));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(cause) if cause.kind() == ErrorKind::AlreadyExists => continue,
            Err(cause) => return Err(cannot("create", &temporary, cause)),
        };
        let written = file
            .write_all(body)
            .and_then(|()| file.set_permissions(metadata.permissions()))
            .and_then(|()| file.sync_all());
        drop(file);
        if let Err(cause) = written {
            let _ = fs::remove_file(&temporary);
            return Err(cannot("write", &temporary, cause));
        }
        return fs::rename(&temporary, &path).map_err(|cause| {
            let _ = fs::remove_file(&temporary);
            cannot("replace", &path, cause)
        });
    }
    fail(format!(
        "cannot create a temporary file beside {}",
        path.display()
    ))
}

fn run() -> Result<()> {
    match parse_args()? {
        Request::Handled => Ok(()),
        Request::Topics => {
            let root = find_root()?;
            let mut text = String::new();
            for name in topic_names(&root)? {
                text.push_str(&name);
                text.push('\n');
            }
            emit(&text)
        }
        Request::Create(args) => create(&args),
    }
}

struct Plan {
    is_new: bool,
    files: Vec<(PathBuf, String)>,
    programs: Vec<String>,
}

impl Plan {
    fn make(args: &Args, root: &Path, registered: bool) -> Result<Self> {
        let topic_dir = root.join(&args.topic);
        let topic_manifest = topic_dir.join("Cargo.toml");
        reject_symlink(&topic_dir, root)?;
        reject_occupied(&topic_dir, root)?;
        reject_symlink(&topic_manifest, root)?;
        let is_new = !topic_manifest.is_file();

        let mut flat = args.flat.clone();
        if flat.is_empty() && args.dirs.is_empty() {
            if is_new && !args.lib {
                flat.push(args.topic.clone());
            } else if !is_new && !args.lib && registered {
                return fail(format!(
                    "{} already exists, name a program to add or pass --lib",
                    args.topic
                ));
            }
        }

        let programs: Vec<String> = flat.iter().chain(&args.dirs).cloned().collect();
        for name in &programs {
            check_name("program", name)?;
        }
        for (i, name) in programs.iter().enumerate() {
            if programs[..i].contains(name) {
                return fail(format!("program '{name}' given twice"));
            }
        }
        let taken = taken_bins(root)?;
        for name in &programs {
            if taken.contains(name) {
                return fail(format!(
                    "binary name '{name}' is already taken in this workspace"
                ));
            }
        }

        let bin = topic_dir.join("src").join("bin");
        let mut files = Vec::new();
        if is_new {
            if topic_manifest.exists() {
                return fail(format!(
                    "{} exists and is not a regular file",
                    relative(&topic_manifest, root)
                ));
            }
            files.push((
                topic_manifest,
                format!(
                    "[package]\nname = \"{}\"\nedition.workspace = true\n",
                    args.topic
                ),
            ));
        }
        if args.lib {
            files.push((topic_dir.join("src").join("lib.rs"), "\n".to_string()));
        }
        for name in &flat {
            files.push((bin.join(format!("{name}.rs")), PROGRAM_STUB.to_string()));
        }
        for name in &args.dirs {
            files.push((bin.join(name).join("main.rs"), PROGRAM_STUB.to_string()));
        }
        for (path, _) in &files {
            if fs::symlink_metadata(path).is_ok() {
                return fail(format!("{} already exists", relative(path, root)));
            }
            for dir in path
                .ancestors()
                .skip(1)
                .take_while(|dir| *dir != topic_dir.as_path())
            {
                reject_symlink(dir, root)?;
            }
        }
        for name in &args.dirs {
            let path = bin.join(name);
            if fs::symlink_metadata(&path).is_ok() {
                return fail(format!("{} already exists", relative(&path, root)));
            }
        }

        Ok(Self {
            is_new,
            files,
            programs,
        })
    }
}

fn warn_roadmap(root: &Path, topic: &str) {
    let roadmap = roadmap_names(root);
    if roadmap.iter().any(|name| name == topic) {
        return;
    }
    match nearest(topic, roadmap.iter().map(String::as_str)) {
        Some(near) => warn(format!(
            "{topic} is not in the .project.toml roadmap, did you mean {near}?"
        )),
        None => warn(format!("{topic} is not in the .project.toml roadmap")),
    }
}

fn verify(root: &Path, topic: &str, plan: &Plan, manifest: &Path, edited: bool) -> Result<()> {
    for (path, _) in &plan.files {
        if !path.exists() {
            return fail(format!("{} was not written", relative(path, root)));
        }
    }
    if edited
        && !member_names(&read_document(manifest)?, manifest)?
            .iter()
            .any(|member| member == topic)
    {
        return fail(format!(
            "{} still does not list \"{topic}\" in workspace.members",
            relative(manifest, root)
        ));
    }
    Ok(())
}

fn execute(
    root: &Path,
    topic: &str,
    plan: &Plan,
    manifest: &Path,
    manifest_bytes: Option<&[u8]>,
) -> Result<()> {
    let mut created = Created::default();
    let outcome = plan
        .files
        .iter()
        .try_for_each(|(path, body)| created.file(path, body))
        .and_then(|()| manifest_bytes.map_or(Ok(()), |bytes| replace_file(manifest, bytes)))
        .and_then(|()| verify(root, topic, plan, manifest, manifest_bytes.is_some()));
    outcome.map_err(|mut error| {
        let wrote = !created.0.is_empty();
        let leftovers = created.undo();
        if wrote && leftovers.is_empty() {
            error
                .message
                .push_str("\n  note: the files written before the failure were removed again");
        }
        for path in leftovers {
            let _ = write!(
                error.message,
                "\n  note: {} could not be removed",
                relative(&path, root)
            );
        }
        error
    })
}

fn report(root: &Path, topic: &str, plan: &Plan, edited: bool) -> String {
    let mut text = String::new();
    for (path, _) in &plan.files {
        let _ = writeln!(text, "  write  {}", relative(path, root));
    }
    if edited {
        let _ = writeln!(text, "  edit   Cargo.toml, adding \"{topic}\" to members");
    }
    text.push_str("\nnext:\n");
    for name in &plan.programs {
        let _ = writeln!(text, "  cargo run -p {topic} --bin {name}");
    }
    text.push_str("  cargo clippy --workspace --all-targets -- -D warnings\n");
    text
}

fn create(args: &Args) -> Result<()> {
    check_name("topic", &args.topic)?;
    if args.topic == "target" {
        return fail("topic 'target' would live in the gitignored build directory");
    }

    let root = find_root()?;
    let _lock = take_lock(&root)?;

    let manifest_path = root.join("Cargo.toml");
    let (mut doc, style) = load(&manifest_path)?;
    let members = member_names(&doc, &manifest_path)?;
    if fs::metadata(&manifest_path).is_ok_and(|metadata| metadata.permissions().readonly()) {
        return fail(format!("{} is read-only", relative(&manifest_path, &root)));
    }

    let registered = members
        .iter()
        .any(|member| as_topic(member) == args.topic.as_str());
    let plan = Plan::make(args, &root, registered)?;
    if plan.files.is_empty() && registered {
        return fail(format!("{} is already set up, nothing to do", args.topic));
    }

    let action = match (plan.is_new, registered) {
        (true, _) => "creating topic",
        (false, false) => "registering topic",
        (false, true) => "extending topic",
    };
    let mut header = format!("{action} {}\n", args.topic);
    if args.dry_run {
        header.push_str("scaffold: dry run, nothing is written\n");
    }
    emit(&header)?;
    if plan.is_new {
        warn_roadmap(&root, &args.topic);
    }

    let manifest_bytes = if registered {
        None
    } else {
        insert_member(&mut doc, &args.topic)?;
        Some(styled(&doc.to_string(), &style))
    };
    if !args.dry_run {
        execute(
            &root,
            &args.topic,
            &plan,
            &manifest_path,
            manifest_bytes.as_deref(),
        )?;
    }
    emit(&report(&root, &args.topic, &plan, manifest_bytes.is_some()))
}
