use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};

use toml_edit::{Array, DocumentMut};

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

One run at a time: the tool takes .scaffold.lock in the repository root for the
whole read and rewrite, so two runs cannot lose each other's entry.

Examples:
  scaffold slice borrowed_view chunks
  scaffold lifetime elision --dir bounds --lib
  scaffold borrow --dry-run
";

const PROGRAM_STUB: &str = "fn main() {}\n";
const NAME_LIMIT: usize = 64;
const LOCK_FILE: &str = ".scaffold.lock";
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

struct Error {
    message: String,
    code: u8,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

type Result<T> = std::result::Result<T, Error>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error {
        message: message.into(),
        code: 1,
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
    raw.into_string().map_err(|raw| Error {
        message: format!("argument {} is not valid UTF-8", raw.to_string_lossy()),
        code: 1,
    })
}

fn parse_args() -> Result<Request> {
    let mut topic: Option<String> = None;
    let mut flat = Vec::new();
    let mut dirs = Vec::new();
    let mut lib = false;
    let mut dry_run = false;

    let mut rest = env::args_os().skip(1);
    while let Some(raw) = rest.next() {
        let arg = text_argument(raw)?;
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(Request::Handled);
            }
            "-t" | "--topics" => return Ok(Request::Topics),
            "-n" | "--dry-run" => dry_run = true,
            "-l" | "--lib" => lib = true,
            "-d" | "--dir" => match rest.next() {
                Some(raw) => dirs.push(text_argument(raw)?),
                None => return fail("--dir needs a name"),
            },
            _ if arg.starts_with('-') => return fail(format!("unknown option {arg}, see --help")),
            _ => match topic {
                None => topic = Some(arg),
                Some(_) => flat.push(arg),
            },
        }
    }

    match topic {
        Some(topic) => Ok(Request::Create(Args {
            topic,
            flat,
            dirs,
            lib,
            dry_run,
        })),
        None => {
            eprint!("{USAGE}");
            Err(Error {
                message: String::new(),
                code: 2,
            })
        }
    }
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

struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn take_lock(root: &Path) -> Result<Lock> {
    let path = root.join(LOCK_FILE);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{}", process::id());
            Ok(Lock(path))
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => fail(format!(
            "another scaffold run holds {}, delete it if no scaffold is running",
            path.display()
        )),
        Err(error) => fail(format!("cannot create {}: {error}", path.display())),
    }
}

struct Style {
    bom: bool,
    crlf: bool,
    final_newline: bool,
}

fn load(path: &Path) -> Result<(DocumentMut, Style)> {
    let bytes = fs::read(path).map_err(|e| Error {
        message: format!("cannot read {}: {e}", path.display()),
        code: 1,
    })?;
    let bom = bytes.starts_with(&BOM);
    let body = if bom { &bytes[BOM.len()..] } else { &bytes[..] };
    let text = std::str::from_utf8(body).map_err(|e| Error {
        message: format!("cannot read {}: {e}", path.display()),
        code: 1,
    })?;
    let style = Style {
        bom,
        crlf: text.contains("\r\n") && !text.replace("\r\n", "").contains('\n'),
        final_newline: text.ends_with('\n'),
    };
    let doc = text.parse::<DocumentMut>().map_err(|e| Error {
        message: format!("cannot parse {}: {e}", path.display()),
        code: 1,
    })?;
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
        .and_then(|workspace| workspace.as_table_like())
        .is_some_and(|workspace| !workspace.is_empty())
}

fn find_root() -> Result<PathBuf> {
    let cwd = env::current_dir().map_err(|e| Error {
        message: format!("cannot read the working directory: {e}"),
        code: 1,
    })?;
    for dir in cwd.ancestors() {
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let Ok(doc) = read_document(&manifest) else {
            continue;
        };
        if is_workspace_root(&doc) {
            return Ok(dir.to_path_buf());
        }
    }
    fail("no workspace manifest above the working directory, run this inside the repository")
}

fn as_topic(entry: &str) -> &str {
    entry
        .trim_start_matches("./")
        .trim_end_matches('/')
        .trim_end_matches("/*")
}

fn member_names(doc: &DocumentMut, path: &Path) -> Result<Vec<String>> {
    let array = workspace_members(doc).ok_or_else(|| Error {
        message: format!("{} has no workspace.members array", path.display()),
        code: 1,
    })?;
    array
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| Error {
                message: format!(
                    "{} has a workspace.members entry that is not a string",
                    path.display()
                ),
                code: 1,
            })
        })
        .collect()
}

fn array_is_multiline(array: &Array) -> bool {
    array
        .iter()
        .filter_map(|value| value.decor().prefix())
        .filter_map(|prefix| prefix.as_str())
        .any(|prefix| prefix.contains('\n'))
        || array.trailing().as_str().is_some_and(|s| s.contains('\n'))
}

fn insert_member(doc: &mut DocumentMut, topic: &str) -> Result<()> {
    let array = doc
        .get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("members"))
        .and_then(|members| members.as_array_mut())
        .ok_or_else(|| Error {
            message: "the root manifest has no workspace.members array".to_string(),
            code: 1,
        })?;

    let multiline = array_is_multiline(array);
    let index = array
        .iter()
        .position(|value| value.as_str().is_some_and(|name| as_topic(name) > topic))
        .unwrap_or(array.len());

    let neighbour = array
        .iter()
        .nth(index.min(array.len().saturating_sub(1)))
        .and_then(|value| value.decor().prefix())
        .and_then(|prefix| prefix.as_str())
        .map(str::to_owned);

    array.insert(index, topic);

    let prefix = match (multiline, index, neighbour) {
        (true, _, Some(prefix)) if prefix.contains('\n') => prefix,
        (true, _, _) => "\n    ".to_string(),
        (false, 0, _) => String::new(),
        (false, _, _) => " ".to_string(),
    };
    if let Some(value) = array.get_mut(index) {
        let decor = value.decor_mut();
        decor.set_prefix(prefix);
        decor.set_suffix("");
    }
    if !multiline
        && index == 0
        && array.len() > 1
        && let Some(value) = array.get_mut(1)
    {
        value.decor_mut().set_prefix(" ");
    }
    Ok(())
}

fn topic_names(root: &Path) -> Result<Vec<String>> {
    let mut names = BTreeSet::new();
    let entries = fs::read_dir(root).map_err(|e| Error {
        message: format!("cannot read {}: {e}", root.display()),
        code: 1,
    })?;
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
    let entries = fs::read_dir(root).map_err(|e| Error {
        message: format!("cannot read {}: {e}", root.display()),
        code: 1,
    })?;

    for entry in entries {
        let dir = entry
            .map_err(|e| Error {
                message: format!("cannot read {}: {e}", root.display()),
                code: 1,
            })?
            .path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let Ok(doc) = read_document(&manifest) else {
            continue;
        };
        let package = doc
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(|name| name.as_str())
            .unwrap_or_default()
            .to_string();

        if !package.is_empty() && dir.join("src").join("main.rs").is_file() {
            bins.insert(package);
        }
        if let Some(tables) = doc.get("bin").and_then(|bin| bin.as_array_of_tables()) {
            for table in tables {
                if let Some(name) = table.get("name").and_then(|name| name.as_str()) {
                    bins.insert(name.to_string());
                }
            }
        }

        let autobins = doc
            .get("package")
            .and_then(|package| package.get("autobins"))
            .and_then(|value| value.as_bool())
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
            eprintln!("scaffold: warning: {error}, skipping the roadmap check");
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
            .filter_map(|entry| entry.as_inline_table())
            .filter_map(|entry| entry.get("name"))
            .filter_map(|name| name.as_str())
            .map(str::to_owned)
            .collect();
    }
    if let Some(tables) = topics.as_array_of_tables() {
        return tables
            .iter()
            .filter_map(|entry| entry.get("name"))
            .filter_map(|name| name.as_str())
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

fn nearest(topic: &str, names: &[String]) -> Option<String> {
    names
        .iter()
        .map(|name| (distance(topic, name), name))
        .filter(|(gap, name)| *gap <= 2 && gap * 3 <= topic.len().max(name.len()))
        .min_by_key(|(gap, _)| *gap)
        .map(|(_, name)| name.clone())
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

fn write_file(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error {
            message: format!("cannot create {}: {e}", parent.display()),
            code: 1,
        })?;
    }
    fs::write(path, body).map_err(|e| Error {
        message: format!("cannot write {}: {e}", path.display()),
        code: 1,
    })
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
    let metadata = fs::metadata(&path).map_err(|e| Error {
        message: format!("cannot read {}: {e}", path.display()),
        code: 1,
    })?;
    if metadata.permissions().readonly() {
        return fail(format!("{} is read-only", path.display()));
    }
    if hard_linked(&metadata) {
        return fs::write(&path, body).map_err(|e| Error {
            message: format!("cannot write {}: {e}", path.display()),
            code: 1,
        });
    }

    let directory = path.parent().unwrap_or(Path::new("."));
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
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return fail(format!("cannot create {}: {error}", temporary.display()));
            }
        };
        let written = file
            .write_all(body)
            .and_then(|()| file.set_permissions(metadata.permissions()))
            .and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = written {
            let _ = fs::remove_file(&temporary);
            return fail(format!("cannot write {}: {error}", temporary.display()));
        }
        return fs::rename(&temporary, &path).map_err(|e| {
            let _ = fs::remove_file(&temporary);
            Error {
                message: format!("cannot replace {}: {e}", path.display()),
                code: 1,
            }
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
            for name in topic_names(&root)? {
                println!("{name}");
            }
            Ok(())
        }
        Request::Create(args) => create(args),
    }
}

fn create(args: Args) -> Result<()> {
    check_name("topic", &args.topic)?;
    if args.topic == "target" {
        return fail("topic 'target' would live in the gitignored build directory");
    }

    let root = find_root()?;
    let _lock = take_lock(&root)?;

    let manifest_path = root.join("Cargo.toml");
    let (mut doc, style) = load(&manifest_path)?;
    let members = member_names(&doc, &manifest_path)?;

    if let Ok(metadata) = fs::metadata(&manifest_path)
        && metadata.permissions().readonly()
    {
        return fail(format!("{} is read-only", relative(&manifest_path, &root)));
    }

    let topic_dir = root.join(&args.topic);
    let topic_manifest = topic_dir.join("Cargo.toml");
    reject_symlink(&topic_dir, &root)?;
    reject_occupied(&topic_dir, &root)?;
    reject_symlink(&topic_manifest, &root)?;

    let is_new = !topic_manifest.is_file();
    let registered = members
        .iter()
        .any(|member| as_topic(member) == args.topic.as_str());

    let mut flat = args.flat;
    let dirs = args.dirs;
    if flat.is_empty() && dirs.is_empty() {
        if is_new && !args.lib {
            flat.push(args.topic.clone());
        } else if !is_new && !args.lib && registered {
            return fail(format!(
                "{} already exists, name a program to add or pass --lib",
                args.topic
            ));
        }
    }

    let new_bins: Vec<&String> = flat.iter().chain(dirs.iter()).collect();
    for name in &new_bins {
        check_name("program", name)?;
    }
    for (i, name) in new_bins.iter().enumerate() {
        if new_bins[..i].contains(name) {
            return fail(format!("program '{name}' given twice"));
        }
    }

    let taken = taken_bins(&root)?;
    for name in &new_bins {
        if taken.contains(name.as_str()) {
            return fail(format!(
                "binary name '{name}' is already taken in this workspace"
            ));
        }
    }

    let mut plan: Vec<(PathBuf, String)> = Vec::new();
    if is_new {
        if topic_manifest.exists() {
            return fail(format!(
                "{} exists and is not a regular file",
                relative(&topic_manifest, &root)
            ));
        }
        plan.push((
            topic_manifest.clone(),
            format!(
                "[package]\nname = \"{}\"\nedition.workspace = true\n",
                args.topic
            ),
        ));
    }
    if args.lib {
        plan.push((topic_dir.join("src").join("lib.rs"), "\n".to_string()));
    }
    for name in &flat {
        plan.push((
            topic_dir.join("src").join("bin").join(format!("{name}.rs")),
            PROGRAM_STUB.to_string(),
        ));
    }
    for name in &dirs {
        plan.push((
            topic_dir.join("src").join("bin").join(name).join("main.rs"),
            PROGRAM_STUB.to_string(),
        ));
    }
    for (path, _) in &plan {
        if path.exists() {
            return fail(format!("{} already exists", relative(path, &root)));
        }
    }
    for name in &dirs {
        let path = topic_dir.join("src").join("bin").join(name);
        if path.exists() {
            return fail(format!("{} already exists", relative(&path, &root)));
        }
    }

    if plan.is_empty() && registered {
        return fail(format!("{} is already set up, nothing to do", args.topic));
    }

    let action = match (is_new, registered) {
        (true, _) => "creating topic",
        (false, false) => "registering topic",
        (false, true) => "extending topic",
    };
    println!("{action} {}", args.topic);
    if args.dry_run {
        println!("scaffold: dry run, nothing is written");
    }

    if is_new {
        let roadmap = roadmap_names(&root);
        if !roadmap.contains(&args.topic) {
            match nearest(&args.topic, &roadmap) {
                Some(near) => eprintln!(
                    "scaffold: warning: {} is not in the .project.toml roadmap, did you mean {}?",
                    args.topic, near
                ),
                None => eprintln!(
                    "scaffold: warning: {} is not in the .project.toml roadmap",
                    args.topic
                ),
            }
        }
    }

    let manifest_bytes = if registered {
        None
    } else {
        insert_member(&mut doc, &args.topic)?;
        Some(styled(&doc.to_string(), &style))
    };

    for (path, body) in &plan {
        if !args.dry_run {
            write_file(path, body)?;
        }
        println!("  write  {}", relative(path, &root));
    }
    if let Some(bytes) = &manifest_bytes {
        if !args.dry_run {
            replace_file(&manifest_path, bytes)?;
        }
        println!("  edit   Cargo.toml, adding \"{}\" to members", args.topic);
    }

    if !args.dry_run {
        for (path, _) in &plan {
            if !path.exists() {
                return fail(format!("{} was not written", relative(path, &root)));
            }
        }
        if manifest_bytes.is_some() {
            let written = member_names(&read_document(&manifest_path)?, &manifest_path)?;
            if !written.contains(&args.topic) {
                return fail(format!(
                    "{} still does not list \"{}\" in workspace.members",
                    relative(&manifest_path, &root),
                    args.topic
                ));
            }
        }
    }

    println!("\nnext:");
    for name in &new_bins {
        println!("  cargo run -p {} --bin {name}", args.topic);
    }
    println!("  cargo clippy --workspace --all-targets -- -D warnings");
    Ok(())
}
