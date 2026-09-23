use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use toml_edit::DocumentMut;

const HEAD: &str = "[workspace]\nresolver = \"3\"\n";
const TAIL: &str = "\n[workspace.package]\nedition = \"2024\"\n";
const MEMBERS: &str = "members = [\"format\", \"variable\"]\n";
const ROADMAP: &str = "\
[taxonomy]
topics = [
  { group = \"basics\", name = \"format\" },
  { group = \"basics\", name = \"function\" },
  { group = \"basics\", name = \"binding\" },
  { group = \"basics\", name = \"slice\" },
  { group = \"basics\", name = \"drop\" },
  { group = \"systems\", name = \"io\" },
  { group = \"systems\", name = \"fs\" },
]
";

struct Repo {
    root: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Repo {
    fn new(name: &str) -> Self {
        Self::with_members(name, MEMBERS)
    }

    fn with_members(name: &str, members: &str) -> Self {
        Self::build(name, format!("{HEAD}{members}{TAIL}").as_bytes(), true)
    }

    fn raw(name: &str, manifest: &[u8]) -> Self {
        Self::build(name, manifest, true)
    }

    fn build(name: &str, manifest: &[u8], crates: bool) -> Self {
        let mut root = env::temp_dir();
        root.push(format!("scaffold-test-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let repo = Self { root };
        fs::write(repo.path("Cargo.toml"), manifest).unwrap();
        repo.set_mode("Cargo.toml", 0o644);
        repo.write(".project.toml", ROADMAP);
        if crates {
            for topic in ["format", "variable"] {
                repo.write(
                    &format!("{topic}/Cargo.toml"),
                    &format!("[package]\nname = \"{topic}\"\nedition.workspace = true\n"),
                );
                repo.write(
                    &format!("{topic}/src/bin/{topic}_prog.rs"),
                    "fn main() {}\n",
                );
            }
        }
        repo
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).unwrap()
    }

    fn exists(&self, relative: &str) -> bool {
        self.path(relative).exists()
    }

    fn mode(&self, relative: &str) -> u32 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(self.path(relative))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        }
        #[cfg(not(unix))]
        {
            let _ = relative;
            0
        }
    }

    fn set_mode(&self, relative: &str, mode: u32) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = self.path(relative);
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(mode);
            fs::set_permissions(&path, permissions).unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = (relative, mode);
        }
    }

    fn members(&self) -> Vec<String> {
        members_of(&self.read("Cargo.toml"))
    }

    fn run(&self, args: &[&str]) -> Run {
        run_in(&self.root, args)
    }

    fn strays(&self) -> Vec<String> {
        fs::read_dir(&self.root)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with(".Cargo.toml.scaffold") || name.ends_with(".lock"))
            .collect()
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_in(root: &Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_scaffold"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn members_of(text: &str) -> Vec<String> {
    text.parse::<DocumentMut>()
        .unwrap()
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(|members| members.as_array())
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect()
}

const SHAPES: &[(&str, &str)] = &[
    ("single_line", "members = [\"format\", \"variable\"]\n"),
    (
        "multi_line",
        "members = [\n    \"format\",\n    \"variable\",\n]\n",
    ),
    (
        "no_last_comma",
        "members = [\n    \"format\",\n    \"variable\"\n]\n",
    ),
    (
        "close_comment",
        "members = [\n    \"format\",\n    \"variable\",\n] # kept sorted\n",
    ),
    (
        "line_comment",
        "members = [\"format\", \"variable\"] # topics\n",
    ),
    ("single_quoted", "members = ['format', 'variable']\n"),
    (
        "inner_comment",
        "members = [\n    # basics\n    \"format\",\n    \"variable\",\n]\n",
    ),
    (
        "odd_indent",
        "members = [\n  \"format\",\n      \"variable\",\n]\n",
    ),
    ("no_space_comma", "members = [\"format\",\"variable\"]\n"),
    (
        "leading_comment",
        "# one member per topic\nmembers = [\"format\", \"variable\"]\n",
    ),
];

#[test]
fn splices_into_every_array_shape() {
    for (name, block) in SHAPES {
        let repo = Repo::with_members(&format!("shape_{name}"), block);
        let run = repo.run(&["functions"]);
        assert_eq!(run.code, 0, "{name}: {}", run.stderr);
        assert_eq!(
            repo.members(),
            ["format", "functions", "variable"],
            "{name}"
        );
    }
}

#[test]
fn keeps_the_existing_order_and_style() {
    let repo = Repo::with_members("order", "members = [\"variable\", \"format\"]\n");
    assert_eq!(repo.run(&["functions"]).code, 0);
    assert_eq!(repo.members(), ["functions", "variable", "format"]);

    let repo = Repo::with_members("style_single", "members = [\"format\", \"variable\"]\n");
    repo.run(&["functions"]);
    assert!(
        repo.read("Cargo.toml")
            .contains("members = [\"format\", \"functions\", \"variable\"]")
    );

    let repo = Repo::with_members(
        "style_multi",
        "members = [\n    \"format\",\n    \"variable\",\n]\n",
    );
    repo.run(&["functions"]);
    assert!(repo.read("Cargo.toml").contains("\n    \"functions\",\n"));
}

#[test]
fn keeps_every_comment_around_and_inside_the_array() {
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "leading",
            "# one member per topic\nmembers = [\"format\", \"variable\"]\n",
            &["# one member per topic"],
        ),
        (
            "groups",
            "members = [\n    # basics\n    \"format\",\n    # data\n    \"variable\",\n]\n",
            &["# basics", "# data"],
        ),
        (
            "per_entry",
            "members = [\n    \"format\",   # printing\n    \"variable\", # bindings\n]\n",
            &["# printing", "# bindings"],
        ),
        (
            "before_close",
            "members = [\n    \"format\",\n    \"variable\",\n    # keep sorted\n]\n",
            &["# keep sorted"],
        ),
    ];
    for (name, block, comments) in cases {
        let repo = Repo::with_members(&format!("comment_{name}"), block);
        let run = repo.run(&["functions"]);
        assert_eq!(run.code, 0, "{name}: {}", run.stderr);
        let text = repo.read("Cargo.toml");
        for comment in *comments {
            assert!(text.contains(comment), "{name} lost {comment}: {text}");
        }
        assert_eq!(
            repo.members(),
            ["format", "functions", "variable"],
            "{name}"
        );
    }
}

#[test]
fn fills_an_empty_array() {
    for (name, block) in [("empty", "members = []\n"), ("spaced", "members = [ ]\n")] {
        let repo = Repo::build(
            &format!("empty_{name}"),
            format!("{HEAD}{block}{TAIL}").as_bytes(),
            false,
        );
        let run = repo.run(&["solo"]);
        assert_eq!(run.code, 0, "{name}: {}", run.stderr);
        assert_eq!(repo.members(), ["solo"], "{name}");
    }
}

#[test]
fn edits_only_the_workspace_members() {
    let repo = Repo::with_members(
        "wrong_table",
        "members = [\"format\", \"variable\"]\n\n[workspace.metadata.release]\nmembers = [\"a\", \"z\"]\n",
    );
    assert_eq!(repo.run(&["functions"]).code, 0);
    assert_eq!(repo.members(), ["format", "functions", "variable"]);
    assert!(repo.read("Cargo.toml").contains("members = [\"a\", \"z\"]"));

    let repo = Repo::with_members(
        "exclude",
        "members = [\n    \"format\",\n    \"variable\",\n]\nexclude = [\"vendor\"]\n",
    );
    repo.run(&["functions"]);
    assert!(repo.read("Cargo.toml").contains("exclude = [\"vendor\"]"));
}

#[test]
fn refuses_a_manifest_without_a_members_array() {
    let repo = Repo::raw(
        "no_members",
        b"[workspace]\nresolver = \"3\"\n\n[workspace.metadata.release]\nmembers = [\"a\"]\n",
    );
    let run = repo.run(&["functions"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("workspace.members"), "{}", run.stderr);
    assert!(!repo.exists("functions"));
}

#[test]
fn refuses_a_members_entry_that_is_not_a_string() {
    for block in [
        "members = [1, \"format\"]\n",
        "members = [[\"a\"], \"format\"]\n",
        "members = \"notanarray\"\n",
    ] {
        let repo = Repo::with_members("not_a_string", block);
        let run = repo.run(&["functions"]);
        assert_ne!(run.code, 0, "{block}");
        assert!(!repo.exists("functions"), "{block}");
    }
}

#[test]
fn keeps_a_glob_entry() {
    let repo = Repo::with_members("glob", "members = [\"format\", \"crates/*\"]\n");
    assert_eq!(repo.run(&["functions"]).code, 0);
    assert_eq!(repo.members(), ["format", "crates/*", "functions"]);
}

#[test]
fn never_duplicates_an_entry() {
    let repo = Repo::with_members(
        "dup",
        "members = [\"format\", \"functions\", \"variable\"]\n",
    );
    let before = repo.read("Cargo.toml");
    let run = repo.run(&["functions"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(repo.read("Cargo.toml"), before);
    assert!(!run.stdout.contains("edit   Cargo.toml"));
    assert!(repo.exists("functions/Cargo.toml"));

    let repo = Repo::with_members("dotted", "members = [\"./format\", \"variable\"]\n");
    let run = repo.run(&["format", "extra"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(repo.members(), ["./format", "variable"]);
}

#[test]
fn registers_a_topic_that_exists_but_is_not_listed() {
    let repo = Repo::new("unlisted");
    repo.write(
        "borrow/Cargo.toml",
        "[package]\nname = \"borrow\"\nedition.workspace = true\n",
    );
    repo.write("borrow/src/bin/borrow.rs", "fn main() {}\n");
    let run = repo.run(&["borrow"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(repo.members(), ["borrow", "format", "variable"]);
}

#[test]
fn writes_a_manifest_for_a_bare_topic_directory() {
    let repo = Repo::new("bare");
    fs::create_dir_all(repo.path("borrow")).unwrap();
    let run = repo.run(&["borrow"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(repo.exists("borrow/Cargo.toml"));
    assert_eq!(repo.members(), ["borrow", "format", "variable"]);
}

#[test]
fn refuses_a_topic_that_is_already_complete() {
    let repo = Repo::with_members(
        "complete",
        "members = [\"format\", \"slice\", \"variable\"]\n",
    );
    repo.write(
        "slice/Cargo.toml",
        "[package]\nname = \"slice\"\nedition.workspace = true\n",
    );
    let run = repo.run(&["slice"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("already exists"), "{}", run.stderr);
}

#[test]
fn the_dry_run_prints_what_the_real_run_prints() {
    let dry = Repo::new("dry");
    let wet = Repo::new("wet");
    let one = dry.run(&["slice", "--dry-run"]);
    let two = wet.run(&["slice"]);
    assert_eq!(one.code, two.code);
    assert_eq!(
        one.stdout
            .replace("scaffold: dry run, nothing is written\n", ""),
        two.stdout
    );
    assert!(one.stdout.contains("next:"));
    assert_eq!(dry.members(), ["format", "variable"]);
    assert!(!dry.exists("slice"));
}

#[test]
fn the_dry_run_agrees_with_the_real_run_on_every_shape() {
    let broken = ("broken", "members = 7\n");
    for (name, block) in SHAPES.iter().chain(std::iter::once(&broken)) {
        let dry = Repo::with_members(&format!("dryshape_{name}"), block);
        let wet = Repo::with_members(&format!("wetshape_{name}"), block);
        assert_eq!(
            dry.run(&["functions", "--dry-run"]).code,
            wet.run(&["functions"]).code,
            "{name}"
        );
    }
}

#[test]
fn refuses_a_binary_name_cargo_already_builds() {
    let repo = Repo::new("bin_explicit");
    repo.write(
        "format/Cargo.toml",
        "[package]\nname = \"format\"\nedition.workspace = true\n\n[[bin]]\nname = \"hello\"\npath = \"src/bin/other.rs\"\n",
    );
    let run = repo.run(&["greeting", "hello"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("already taken"), "{}", run.stderr);

    let repo = Repo::new("bin_package_name");
    repo.write(
        "gamma/Cargo.toml",
        "[package]\nname = \"realname\"\nedition.workspace = true\n",
    );
    repo.write("gamma/src/main.rs", "fn main() {}\n");
    let run = repo.run(&["delta", "realname"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("already taken"), "{}", run.stderr);
}

#[test]
fn leaves_free_names_free() {
    let repo = Repo::new("bin_helpers");
    repo.write("format/src/bin/helpers/util.rs", "pub fn f() {}\n");
    let run = repo.run(&["slice", "helpers"]);
    assert_eq!(run.code, 0, "{}", run.stderr);

    let repo = Repo::new("bin_autobins");
    repo.write(
        "format/Cargo.toml",
        "[package]\nname = \"format\"\nedition.workspace = true\nautobins = false\n",
    );
    let run = repo.run(&["slice", "format_prog"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
}

#[test]
fn refuses_a_program_named_twice() {
    let repo = Repo::new("twice");
    let run = repo.run(&["slice", "one", "one"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("given twice"), "{}", run.stderr);
}

#[test]
fn rejects_names_it_cannot_write() {
    let repo = Repo::new("names");
    for bad in ["Struct", "1st", "with-dash", "", "../etc", &"a".repeat(65)] {
        assert_ne!(repo.run(&[bad]).code, 0, "{bad}");
    }
    let run = repo.run(&["target"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("gitignored"), "{}", run.stderr);
    assert_eq!(repo.run(&["a".repeat(64).as_str()]).code, 0);
}

#[test]
fn reports_a_missing_topic_with_exit_code_two() {
    let repo = Repo::new("no_topic");
    let run = repo.run(&[]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("Usage: scaffold"));
    assert!(!run.stderr.trim_end().ends_with("no topic given"));
}

#[test]
fn keeps_the_manifest_mode_and_leaves_no_temporary_file() {
    let repo = Repo::new("mode");
    let before = repo.mode("Cargo.toml");
    assert_eq!(repo.run(&["functions"]).code, 0);
    assert_eq!(repo.mode("Cargo.toml"), before);
    assert!(repo.strays().is_empty(), "{:?}", repo.strays());
}

#[test]
fn does_not_clobber_a_file_named_like_the_temporary() {
    let repo = Repo::new("temp_collision");
    repo.write("Cargo.toml.scaffold", "IMPORTANT USER DATA\n");
    assert_eq!(repo.run(&["functions"]).code, 0);
    assert_eq!(repo.read("Cargo.toml.scaffold"), "IMPORTANT USER DATA\n");
}

#[test]
fn ignores_an_unparseable_sibling_manifest() {
    let repo = Repo::new("junk");
    repo.write("junk/Cargo.toml", "this is not = = toml\n");
    let run = repo.run(&["functions"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(repo.members(), ["format", "functions", "variable"]);
    assert!(run.stderr.contains("warning"), "{}", run.stderr);
    assert!(run.stderr.contains("junk/Cargo.toml"), "{}", run.stderr);
    assert!(run.stderr.contains("not checked"), "{}", run.stderr);
}

#[test]
fn refuses_an_occupied_topic_path() {
    let repo = Repo::new("occupied_file");
    repo.write("slice", "x");
    let run = repo.run(&["slice"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("not a directory"), "{}", run.stderr);
    assert_eq!(repo.members(), ["format", "variable"]);

    let repo = Repo::new("occupied_manifest");
    fs::create_dir_all(repo.path("slice/Cargo.toml")).unwrap();
    let run = repo.run(&["slice"]);
    assert_ne!(run.code, 0);
    assert!(!repo.exists("slice/src"));
}

#[test]
fn a_library_topic_gets_no_program() {
    let repo = Repo::new("lib_only");
    let run = repo.run(&["slice", "--lib"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(repo.exists("slice/src/lib.rs"));
    assert!(!repo.exists("slice/src/bin"));

    let repo = Repo::new("lib_and_program");
    assert_eq!(repo.run(&["slice", "view", "--lib"]).code, 0);
    assert!(repo.exists("slice/src/lib.rs"));
    assert!(repo.exists("slice/src/bin/view.rs"));
}

#[test]
fn preserves_a_byte_order_mark_crlf_and_a_missing_final_newline() {
    let crlf = format!("{HEAD}{MEMBERS}{TAIL}").replace('\n', "\r\n");
    let repo = Repo::raw("crlf", crlf.as_bytes());
    assert_eq!(repo.run(&["functions"]).code, 0);
    let text = repo.read("Cargo.toml");
    assert!(!text.contains('\n') || text.contains("\r\n"));
    assert_eq!(text.matches('\n').count(), text.matches("\r\n").count());
    assert_eq!(members_of(&text), ["format", "functions", "variable"]);

    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(format!("{HEAD}{MEMBERS}{TAIL}").as_bytes());
    let repo = Repo::raw("bom", &bom);
    assert_eq!(repo.run(&["functions"]).code, 0);
    let bytes = fs::read(repo.path("Cargo.toml")).unwrap();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);

    let trimmed = format!("{HEAD}{MEMBERS}{TAIL}").trim_end().to_string();
    let repo = Repo::raw("no_final_newline", trimmed.as_bytes());
    assert_eq!(repo.run(&["functions"]).code, 0);
    assert!(!repo.read("Cargo.toml").ends_with('\n'));
}

#[test]
fn lists_topics_for_shell_completion() {
    let repo = Repo::new("topics");
    let run = repo.run(&["--topics"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let names: Vec<&str> = run.stdout.lines().collect();
    assert!(names.contains(&"format"));
    assert!(names.contains(&"variable"));
    assert!(names.contains(&"function"));
    assert!(names.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn warns_only_about_topics_the_roadmap_really_lacks() {
    let repo = Repo::new("roadmap");
    assert!(
        !repo
            .run(&["slice", "--dry-run"])
            .stderr
            .contains("not in the .project.toml roadmap")
    );
    let run = repo.run(&["functons", "--dry-run"]);
    assert!(
        run.stderr.contains("did you mean function?"),
        "{}",
        run.stderr
    );
    let run = repo.run(&["sp", "--dry-run"]);
    assert!(!run.stderr.contains("did you mean"), "{}", run.stderr);

    let repo = Repo::new("roadmap_tables");
    repo.write(
        ".project.toml",
        "[taxonomy]\n[[taxonomy.topics]]\nname = \"slice\"\n",
    );
    assert!(
        !repo
            .run(&["slice", "--dry-run"])
            .stderr
            .contains("not in the .project.toml roadmap")
    );

    let repo = Repo::new("roadmap_broken");
    repo.write(".project.toml", "[taxonomy\ntopics = broken\n");
    let run = repo.run(&["slice", "--dry-run"]);
    assert_eq!(run.code, 0);
    assert!(
        run.stderr.contains("skipping the roadmap check"),
        "{}",
        run.stderr
    );
}

#[test]
fn serialises_concurrent_runs() {
    let repo = Repo::new("concurrent");
    let root = &repo.root;
    let topics = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    ];
    let results: Vec<(&str, Run)> = std::thread::scope(|scope| {
        let handles: Vec<_> = topics
            .iter()
            .map(|topic| scope.spawn(move || (*topic, run_in(root, &[topic]))))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let members = repo.members();
    for (topic, run) in &results {
        assert_eq!(run.code, 0, "{topic}: {}", run.stderr);
        assert!(
            members.iter().any(|member| member == topic),
            "{topic} reported success but is missing from {members:?}"
        );
    }
    assert!(repo.strays().is_empty(), "{:?}", repo.strays());
}

#[cfg(unix)]
#[test]
fn follows_a_symlinked_manifest_instead_of_replacing_it() {
    let repo = Repo::new("symlink_manifest");
    fs::rename(repo.path("Cargo.toml"), repo.path("workspace.toml")).unwrap();
    std::os::unix::fs::symlink("workspace.toml", repo.path("Cargo.toml")).unwrap();
    let run = repo.run(&["functions"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(
        fs::symlink_metadata(repo.path("Cargo.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        members_of(&repo.read("workspace.toml")),
        ["format", "functions", "variable"]
    );
}

#[cfg(unix)]
#[test]
fn keeps_a_hard_linked_manifest_linked() {
    let repo = Repo::new("hardlink");
    fs::hard_link(repo.path("Cargo.toml"), repo.path("Cargo.toml.backup")).unwrap();
    assert_eq!(repo.run(&["functions"]).code, 0);
    assert_eq!(
        members_of(&repo.read("Cargo.toml.backup")),
        ["format", "functions", "variable"]
    );
}

#[cfg(unix)]
#[test]
fn refuses_to_write_through_a_symlinked_topic() {
    let repo = Repo::new("symlink_topic");
    let outside = repo.path("outside");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, repo.path("escaped")).unwrap();
    let run = repo.run(&["escaped"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("symlink"), "{}", run.stderr);
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);

    let repo = Repo::new("symlink_topic_manifest");
    fs::create_dir_all(repo.path("dang")).unwrap();
    let hijack = repo.path("hijacked.toml");
    std::os::unix::fs::symlink(&hijack, repo.path("dang/Cargo.toml")).unwrap();
    let run = repo.run(&["dang"]);
    assert_ne!(run.code, 0);
    assert!(!hijack.exists());
}

#[cfg(unix)]
#[test]
fn refuses_a_read_only_manifest() {
    let repo = Repo::new("readonly");
    repo.set_mode("Cargo.toml", 0o444);
    let run = repo.run(&["functions"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("read-only"), "{}", run.stderr);
    assert!(!repo.exists("functions"));
    assert_eq!(repo.members(), ["format", "variable"]);
    repo.set_mode("Cargo.toml", 0o644);
}

#[cfg(unix)]
#[test]
fn rejects_a_non_utf8_argument() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let repo = Repo::new("non_utf8");
    let output = Command::new(env!("CARGO_BIN_EXE_scaffold"))
        .arg(OsStr::from_bytes(b"\xff\xfe"))
        .current_dir(&repo.root)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(stderr.contains("not valid UTF-8"), "{stderr}");
}

#[test]
fn cargo_resolves_what_it_wrote() {
    let repo = Repo::with_members(
        "cargo",
        "members = [\n    \"format\",\n    \"variable\",\n]\n",
    );
    assert_eq!(
        repo.run(&["functions", "--dir", "spanning", "--lib"]).code,
        0
    );
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&repo.root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8_lossy(&output.stdout);
    for expected in ["\"functions\"", "\"spanning\""] {
        assert!(json.contains(expected), "cargo did not report {expected}");
    }
}

#[test]
fn keeps_each_comment_once_and_on_its_line() {
    let cases: &[(&str, &str, &str, &[&str])] = &[
        (
            "trailing_between",
            "members = [\n    \"format\",   # printing\n    \"variable\", # bindings\n]\n",
            "functions",
            &[
                "    \"format\",   # printing\n",
                "    \"functions\",\n",
                "    \"variable\", # bindings\n",
            ],
        ),
        (
            "header_first",
            "members = [\n    # basics\n    \"format\",\n    \"variable\",\n]\n",
            "alpha",
            &["    \"alpha\",\n", "    # basics\n    \"format\",\n"],
        ),
        (
            "header_last",
            "members = [\n    \"format\",\n    # misc\n    \"variable\",\n]\n",
            "zeta",
            &["    # misc\n    \"variable\",\n", "    \"zeta\",\n]"],
        ),
        (
            "trailing_last_no_comma",
            "members = [\n    \"format\",\n    \"variable\" # bindings\n]\n",
            "zeta",
            &["    \"variable\", # bindings\n", "    \"zeta\"\n]"],
        ),
        (
            "trailing_last_comma",
            "members = [\n    \"format\",\n    \"variable\", # bindings\n]\n",
            "zeta",
            &["    \"variable\", # bindings\n", "    \"zeta\",\n]"],
        ),
        (
            "blank_line_groups",
            "members = [\n    \"format\",\n\n    # data\n    \"variable\",\n]\n",
            "functions",
            &["    \"functions\",\n\n    # data\n    \"variable\",\n"],
        ),
    ];
    for (name, block, topic, lines) in cases {
        let repo = Repo::with_members(&format!("once_{name}"), block);
        let run = repo.run(&[topic]);
        assert_eq!(run.code, 0, "{name}: {}", run.stderr);
        let text = repo.read("Cargo.toml");
        for line in *lines {
            assert_eq!(text.matches(line).count(), 1, "{name}: {line:?} in {text}");
        }
        assert_eq!(
            text.matches('#').count(),
            block.matches('#').count(),
            "{name}: {text}"
        );
        assert!(
            repo.members().iter().any(|member| member == topic),
            "{name}"
        );
    }
}

#[test]
fn explains_why_no_workspace_root_was_found() {
    let cases: &[(&str, &[u8], &str)] = &[
        (
            "package_only",
            b"[package]\nname = \"x\"\nedition = \"2024\"\n",
            "no [workspace] table",
        ),
        (
            "empty_workspace",
            b"[workspace]\n",
            "empty [workspace] table",
        ),
        (
            "unparseable",
            b"[workspace\nmembers = [\n",
            "TOML parse error",
        ),
    ];
    for (name, manifest, note) in cases {
        let repo = Repo::raw(&format!("root_{name}"), manifest);
        let run = repo.run(&["slice"]);
        assert_ne!(run.code, 0, "{name}");
        assert!(
            run.stderr.contains("no workspace manifest above"),
            "{name}: {}",
            run.stderr
        );
        assert!(run.stderr.contains("note: "), "{name}: {}", run.stderr);
        assert!(run.stderr.contains(note), "{name}: {}", run.stderr);
        assert!(!repo.exists("slice"), "{name}");
    }
}

#[test]
fn hints_at_the_right_option_syntax() {
    let repo = Repo::new("hints");
    let cases: &[(&[&str], &str)] = &[
        (&["slice", "--dir=view"], "--dir view"),
        (&["slice", "--dir="], "--dir <name>"),
        (&["slice", "--lib=yes"], "--lib takes no value"),
        (&["slice", "--libs"], "did you mean --lib?"),
        (&["slice", "-lib"], "did you mean --lib?"),
        (&["slice", "-nl"], "-n -l"),
        (&["slice", "--dir", "--lib"], "needs a name, found --lib"),
        (&["slice", "--dir"], "needs a name"),
        (&["slice", "--frob"], "see --help"),
    ];
    for (args, hint) in cases {
        let run = repo.run(args);
        assert_ne!(run.code, 0, "{args:?}");
        assert!(run.stderr.contains(hint), "{args:?}: {}", run.stderr);
        assert!(!repo.exists("slice"), "{args:?}");
    }
    assert_eq!(repo.members(), ["format", "variable"]);
}

#[test]
fn recovers_from_a_stale_lock_file() {
    let repo = Repo::new("stale_lock");
    repo.write(".scaffold.lock", "999999\n");
    let run = repo.run(&["functions"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(repo.members(), ["format", "functions", "variable"]);
    assert!(repo.strays().is_empty(), "{:?}", repo.strays());
}

#[cfg(unix)]
#[test]
fn refuses_to_write_through_a_dangling_symlink() {
    let repo = Repo::new("dangling");
    repo.write(
        "slice/Cargo.toml",
        "[package]\nname = \"slice\"\nedition.workspace = true\n",
    );
    fs::create_dir_all(repo.path("slice/src/bin")).unwrap();
    let outside = repo.path("outside");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(
        outside.join("hijack.rs"),
        repo.path("slice/src/bin/view.rs"),
    )
    .unwrap();
    let run = repo.run(&["slice", "view"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("already exists"), "{}", run.stderr);
    assert!(!outside.join("hijack.rs").exists());
    assert_eq!(repo.members(), ["format", "variable"]);

    let repo = Repo::new("symlinked_src");
    repo.write(
        "slice/Cargo.toml",
        "[package]\nname = \"slice\"\nedition.workspace = true\n",
    );
    let outside = repo.path("outside");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, repo.path("slice/src")).unwrap();
    let run = repo.run(&["slice", "view"]);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("symlink"), "{}", run.stderr);
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    assert_eq!(repo.members(), ["format", "variable"]);
}

#[cfg(unix)]
#[test]
fn removes_what_it_wrote_when_a_later_write_fails() {
    let repo = Repo::new("rollback");
    fs::create_dir_all(repo.path("slice/src/bin")).unwrap();
    repo.set_mode("slice/src/bin", 0o555);
    if fs::write(repo.path("slice/src/bin/probe"), "").is_ok() {
        return;
    }
    let run = repo.run(&["slice", "view"]);
    repo.set_mode("slice/src/bin", 0o755);
    assert_ne!(run.code, 0);
    assert!(run.stderr.contains("cannot write"), "{}", run.stderr);
    assert!(run.stderr.contains("removed again"), "{}", run.stderr);
    assert!(!run.stdout.contains("write  "), "{}", run.stdout);
    assert!(!repo.exists("slice/Cargo.toml"));
    assert!(repo.exists("slice/src/bin"));
    assert_eq!(repo.members(), ["format", "variable"]);
    assert!(repo.strays().is_empty(), "{:?}", repo.strays());
}
