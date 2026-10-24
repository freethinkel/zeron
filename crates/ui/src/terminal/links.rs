//! Links in terminal rows, for Cmd+click (Ctrl+click off macOS): URLs and
//! `file:line` paths found in the text, and what an OSC 8 hyperlink opens.
//! Pure: rows in as chars, char ranges out.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::Modifiers;

use super::emulator::CellSnapshot;

#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    Url(String),
    File {
        path: String,
        line: Option<u32>,
        col: Option<u32>,
    },
}

/// What turns a click into opening a link: Cmd on macOS, Ctrl elsewhere.
pub fn link_modifier(mods: &Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        mods.platform
    } else {
        mods.control
    }
}

/// What an OSC 8 hyperlink opens: web and mail links as they are, a
/// `file://` one as its path (`ls --hyperlink` names the host and
/// percent-encodes the path). Other schemes open nothing: the link's text
/// hides where they lead.
pub fn hyperlink(uri: &str) -> Option<Link> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let url = url::Url::parse(&format!("file://{}", &rest[rest.find('/')?..])).ok()?;
        let path = url.to_file_path().ok()?;
        return Some(Link::File {
            path: path.to_string_lossy().into_owned(),
            line: None,
            col: None,
        });
    }
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|s| uri.starts_with(s))
        .then(|| Link::Url(uri.into()))
}

/// The URL or path (`src/a.rs:12:5`) under char `at`, with its char range in
/// `line`.
// ponytail: one row; a URL wrapped onto the next row opens cut short.
pub fn link_at(line: &[char], at: usize) -> Option<(Range<usize>, Link)> {
    let stop = |c: char| c.is_whitespace() || "\"'`<>()[]{}|│".contains(c);
    if at >= line.len() || stop(line[at]) {
        return None;
    }
    let mut start = at;
    while start > 0 && !stop(line[start - 1]) {
        start -= 1;
    }
    let mut end = at + 1;
    while end < line.len() && !stop(line[end]) {
        end += 1;
    }
    let token: String = line[start..end].iter().collect();
    let punctuation = ['.', ',', ';', ':', '!', '?'];
    // `url=https://…`: the link starts at its scheme.
    if let Some(i) = ["https://", "http://", "file://"]
        .iter()
        .filter_map(|s| token.find(s))
        .min()
    {
        let url = token[i..].trim_end_matches(punctuation);
        let from = start + token[..i].chars().count();
        let range = from..from + url.chars().count();
        let link = match url.strip_prefix("file://") {
            Some(_) => hyperlink(url)?,
            None => Link::Url(url.to_string()),
        };
        return range.contains(&at).then_some((range, link));
    }
    let trimmed = token.trim_end_matches(punctuation);
    let mut path = trimmed;
    let mut nums = Vec::new();
    while let Some((rest, n)) = path
        .rsplit_once(':')
        .and_then(|(rest, n)| n.parse::<u32>().ok().map(|n| (rest, n)))
    {
        if nums.len() == 2 {
            break;
        }
        nums.insert(0, n);
        path = rest;
    }
    // A path has a slash, or is a file name with a line (`main.rs:3`);
    // "and/or" passes too, opening it finds nothing.
    let file_name = path.contains('.') && !nums.is_empty();
    if path.is_empty() || !(path.contains('/') || file_name) || path.contains("://") {
        return None;
    }
    let range = start..start + trimmed.chars().count();
    range.contains(&at).then(|| {
        (
            range,
            Link::File {
                path: path.to_string(),
                line: nums.first().copied(),
                col: nums.get(1).copied(),
            },
        )
    })
}

/// A row's chars, wide-char spacers left out, and the cell each one is in.
pub fn row_chars(cells: &[CellSnapshot]) -> (Vec<char>, Vec<usize>) {
    cells
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.wide_spacer)
        .map(|(i, c)| (c.ch, i))
        .unzip()
}

/// A char range of [`row_chars`] as cells.
pub fn cell_range(cells: &[CellSnapshot], cols: &[usize], chars: Range<usize>) -> Range<usize> {
    let last = cols[chars.end - 1];
    cols[chars.start]..last + 1 + usize::from(cells[last].wide)
}

/// Where a path from the screen points: `~/` under `home`, a relative one
/// under the shell's `cwd`, when they are known.
pub fn resolve(path: &str, cwd: Option<&Path>, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~/"), home, cwd) {
        (Some(rest), Some(home), _) => home.join(rest),
        (None, _, Some(cwd)) => cwd.join(path),
        _ => PathBuf::from(path),
    }
}

/// A path and its location as a chat's file link: `path:line:col`, an
/// absolute path as `file://`, so a name with no extension (`Makefile`)
/// still counts as a file.
pub fn file_link_target(path: &Path, line: Option<u32>, col: Option<u32>) -> String {
    let mut target = path.to_string_lossy().into_owned();
    if path.is_absolute() {
        target.insert_str(0, "file://");
    }
    for n in [line, col].into_iter().map_while(|n| n) {
        target.push_str(&format!(":{n}"));
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn finds_urls_and_paths_under_the_pointer() {
        let line = chars("see https://zeron.sh/docs. and (src/main.rs:12:5) then ~/x/y.txt, ok");
        let url = link_at(&line, 10).unwrap();
        assert_eq!(url.1, Link::Url("https://zeron.sh/docs".into()));
        assert_eq!(url.0, 4..25);
        let file = Link::File {
            path: "src/main.rs".into(),
            line: Some(12),
            col: Some(5),
        };
        assert_eq!(link_at(&line, 35), Some((32..48, file)));
        assert_eq!(
            link_at(&line, 57).map(|l| l.1),
            Some(Link::File {
                path: "~/x/y.txt".into(),
                line: None,
                col: None
            })
        );
        assert_eq!(link_at(&line, 0), None); // "see"
        assert_eq!(link_at(&line, 3), None); // a space
    }

    #[test]
    fn a_bare_file_name_needs_a_line() {
        assert_eq!(
            link_at(&chars("lib.rs:7:"), 0).map(|l| l.1),
            Some(Link::File {
                path: "lib.rs".into(),
                line: Some(7),
                col: None
            })
        );
        assert_eq!(link_at(&chars("lib.rs"), 0), None);
        assert_eq!(link_at(&chars("12:30"), 0), None);
    }

    #[test]
    fn a_url_after_a_prefix_starts_at_its_scheme() {
        let line = chars("url=http://a.b/c");
        assert_eq!(link_at(&line, 1), None);
        assert_eq!(
            link_at(&line, 8),
            Some((4..16, Link::Url("http://a.b/c".into())))
        );
    }

    #[test]
    fn hyperlinks_open_web_mail_and_local_files_only() {
        let url = |s: &str| Some(Link::Url(s.into()));
        let file = |s: &str| {
            Some(Link::File {
                path: s.into(),
                line: None,
                col: None,
            })
        };
        assert_eq!(hyperlink("https://a.b/c"), url("https://a.b/c"));
        assert_eq!(hyperlink("mailto:x@a.b"), url("mailto:x@a.b"));
        if cfg!(unix) {
            assert_eq!(
                hyperlink("file://mac.local/Users/x/a%20b"),
                file("/Users/x/a b")
            );
            assert_eq!(hyperlink("file:///tmp"), file("/tmp"));
        }
        assert_eq!(hyperlink("ssh://host"), None);
    }

    /// Cells of a char range: a wide char covers its spacer too.
    #[test]
    fn a_char_range_covers_wide_cells() {
        let mut e = super::super::emulator::Emulator::new(20, 2);
        e.feed("宽 a/b.rs:1".as_bytes());
        let cells = e.line(0);
        let (chars, cols) = row_chars(&cells);
        assert_eq!(chars.iter().collect::<String>().trim_end(), "宽 a/b.rs:1");
        let (range, _) = link_at(&chars, 3).unwrap();
        assert_eq!(cell_range(&cells, &cols, range), 3..11);
        assert_eq!(cell_range(&cells, &cols, 0..1), 0..2);
    }

    #[test]
    fn paths_resolve_against_home_and_the_shell_directory() {
        let (cwd, home) = (Path::new("/repo/src"), Path::new("/home/me"));
        assert_eq!(
            resolve("~/x.txt", Some(cwd), Some(home)),
            Path::new("/home/me/x.txt")
        );
        assert_eq!(
            resolve("a/b.rs", Some(cwd), Some(home)),
            Path::new("/repo/src/a/b.rs")
        );
        assert_eq!(
            resolve("/etc/hosts", Some(cwd), None),
            Path::new("/etc/hosts")
        );
        assert_eq!(resolve("a/b.rs", None, None), Path::new("a/b.rs"));
        assert_eq!(resolve("~/x.txt", None, None), Path::new("~/x.txt"));
    }

    /// The targets read as the chat's file links: inside its checkout, with
    /// the location, extension or not.
    #[cfg(unix)]
    #[test]
    fn file_link_targets_resolve_as_chat_file_links() {
        use crate::workspace_links::{FileLinkResolution, first_root_owning};
        let owned = |target: &str| match first_root_owning(target, ["/repo"]) {
            Some(FileLinkResolution::Owned { root: 0, link }) => {
                (link.path, link.line, link.column)
            }
            other => panic!("{target}: {other:?}"),
        };
        let makefile = file_link_target(Path::new("/repo/Makefile"), Some(3), Some(2));
        assert_eq!(owned(&makefile), ("Makefile".into(), Some(3), Some(2)));
        let relative = file_link_target(Path::new("src/a.rs"), Some(9), None);
        assert_eq!(owned(&relative), ("src/a.rs".into(), Some(9), None));
    }

    #[test]
    fn file_link_targets_carry_the_location() {
        assert_eq!(
            file_link_target(Path::new("src/a.rs"), Some(3), Some(7)),
            "src/a.rs:3:7"
        );
        assert_eq!(file_link_target(Path::new("a.rs"), Some(3), None), "a.rs:3");
        if cfg!(unix) {
            assert_eq!(
                file_link_target(Path::new("/repo/Makefile"), None, None),
                "file:///repo/Makefile"
            );
        }
    }
}
