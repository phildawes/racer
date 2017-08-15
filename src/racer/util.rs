// Small functions of utility
use std::cmp;
use std::path;
use std::rc::Rc;

use core::{IndexedSource, Session, SessionExt, Location, LocationExt, Point};

use core::SearchType::{self, ExactMatch, StartsWith};

pub fn is_pattern_char(c: char) -> bool {
    c.is_alphanumeric() || c.is_whitespace() || (c == '_') || (c == ':') || (c == '.')
}

pub fn is_search_expr_char(c: char) -> bool {
    c.is_alphanumeric() || (c == '_') || (c == ':') || (c == '.')
}

pub fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || (c == '_') || (c == '!')
}

/// Searches for `needle` as a standalone identifier in `haystack`. To be considered a match,
/// the `needle` must occur either at the beginning of `haystack` or after a non-identifier
/// character.
pub fn txt_matches(stype: SearchType, needle: &str, haystack: &str) -> bool {
    match stype {
        ExactMatch => {
            let n_len = needle.len();
            let h_len = haystack.len();

            if n_len == 0 {
                return true;
            }

            // PD: switch to use .match_indices() when that stabilizes
            let mut n=0;
            while let Some(n1) = haystack[n..].find(needle) {
                n += n1;
                if (n == 0  || !is_ident_char(char_at(haystack, n-1))) &&
                    (n+n_len == h_len || !is_ident_char(char_at(haystack, n+n_len))) {
                    return true;
                }
                n += 1;
            }
            false
        },
        StartsWith => {
            if needle.is_empty() {
                return true;
            }

            // PD: switch to use .match_indices() when that stabilizes
            let mut n=0;
            while let Some(n1) = haystack[n..].find(needle) {
                n += n1;
                if n == 0  || !is_ident_char(char_at(haystack, n-1)) {
                    return true;
                }
                n += 1;
            }
            false
        }
    }
}

pub fn symbol_matches(stype: SearchType, searchstr: &str, candidate: &str) -> bool {
   match stype {
        ExactMatch => searchstr == candidate,
        StartsWith => candidate.starts_with(searchstr)
    }
}

/// Try to valid if the given scope contains a valid closure arg scope.
pub fn closure_valid_arg_scope(scope_src: &str) -> Option<(usize, usize, &str)> {
    // Try to find the left and right pipe, if one or both are not present, this is not a valid
    // closure definition
    let left_pipe = if let Some(pos) = scope_src.find('|') { pos } else { return None; };
    let rest_scope = &scope_src[left_pipe + 1..];
    let right_pipe = if let Some(pos) = rest_scope.find('|') {
        left_pipe + 1 + pos
    } else {
        return None;
    };

    let pipe_scope = &scope_src[left_pipe..right_pipe+1];

    // For each '{' increase the counter by one and for each '}' decrease the counter by one
    // If we have a equal number of curly brackets, we should get 0 as result
    let curly_brackets = pipe_scope.chars().fold(0,
                           |count, c| {
                               if c == '{' {
                                   count + 1
                               } else if c == '}' {
                                   count - 1
                               } else {
                                   count
                               }
                           });

    // If we found an unequal number of curly brackets in the scope, this can not be a valid
    // closure definition
    if curly_brackets != 0 {
        return None;
    }

    // If we find a ';' --> no closure definition
    if pipe_scope.contains(';') {
        return None;
    }

    Some((left_pipe, right_pipe, pipe_scope))
}

// pub fn get_backtrace() -> String {
//     let mut m = std::old_io::MemWriter::new();
//     let s = std::rt::backtrace::write(&mut m)
//         .ok().map_or("NO backtrace".to_string(),
//                      |_| String::from_utf8_lossy(m.get_ref()).to_string());
//     return s;
// }

#[test]
fn txt_matches_matches_stuff() {
    assert_eq!(true, txt_matches(ExactMatch, "Vec","Vec"));
    assert_eq!(true, txt_matches(StartsWith, "Vec","Vector"));
    assert_eq!(false, txt_matches(ExactMatch, "Vec","use Vector"));
    assert_eq!(true, txt_matches(StartsWith, "Vec","use Vector"));
    assert_eq!(false, txt_matches(StartsWith, "Vec","use aVector"));
    assert_eq!(true, txt_matches(ExactMatch, "Vec","use Vec"));
}

#[test]
fn txt_matches_matches_methods() {
    assert_eq!(true, txt_matches(StartsWith, "do_st", "fn do_stuff"));
    assert_eq!(true, txt_matches(StartsWith, "do_st", "pub fn do_stuff"));
    assert_eq!(true, txt_matches(StartsWith, "do_st", "pub(crate) fn do_stuff"));
    assert_eq!(true, txt_matches(StartsWith, "do_st", "pub(in codegen) fn do_stuff"));
}


/// Given a string and index, return span of identifier
///
/// `pos` is coerced to be within `s`. Note that `expand_ident` only backtracks.
/// If the provided `pos` is in the middle of an identifier, the returned
/// `(start, end)` will have `end` = `pos`.
///
/// # Examples
///
/// ```
/// extern crate racer;
///
/// let src = "let x = this_is_an_identifier;";
/// let pos = racer::Location::Point(29);
/// let path = "lib.rs";
///
/// let cache = racer::FileCache::default();
/// let session = racer::Session::new(&cache);
///
/// session.cache_file_contents(path, src);
///
/// let expanded = racer::expand_ident(path, pos, &session).unwrap();
/// assert_eq!("this_is_an_identifier", expanded.ident());
/// ```
pub fn expand_ident<P, C>(
    filepath: P,
    cursor: C,
    session: &Session
) -> Option<ExpandedIdent>
    where P: AsRef<::std::path::Path>,
          C: Into<Location>
{
    let cursor = cursor.into();
    let indexed_source = session.load_file(filepath.as_ref());
    let (start, pos) = {
        let s = &indexed_source.code[..];
        let pos = match cursor.to_point(&indexed_source) {
            Some(pos) => pos,
            None => {
                debug!("Failed to convert cursor to point");
                return None;
            }
        };

        // TODO: Would this better be an assertion ? Why are out-of-bound values getting here ?
        // They are coming from the command-line, question is, if they should be handled beforehand
        // clamp pos into allowed range
        let pos = cmp::min(s.len(), pos);
        let sb = &s[..pos];
        let mut start = pos;

        // backtrack to find start of word
        for (i, c) in sb.char_indices().rev() {
            if !is_ident_char(c) {
                break;
            }
            start = i;
        }

        (start, pos)
    };

    Some(ExpandedIdent {
        src: indexed_source,
        start: start,
        pos: pos,
    })
}

pub struct ExpandedIdent {
    src: Rc<IndexedSource>,
    start: Point,
    pos: Point,
}

impl ExpandedIdent {
    pub fn ident(&self) -> &str {
        &self.src.code[self.start..self.pos]
    }

    pub fn start(&self) -> Point {
        self.start
    }

    pub fn pos(&self) -> Point {
        self.pos
    }
}

pub fn find_ident_end(s: &str, pos: Point) -> Point {
    // find end of word
    let sa = &s[pos..];
    for (i, c) in sa.char_indices() {
        if !is_ident_char(c) {
            return pos + i;
        }
    }
    s.len()
}

#[test]
fn find_ident_end_ascii() {
    assert_eq!(5, find_ident_end("ident", 0));
    assert_eq!(6, find_ident_end("(ident)", 1));
    assert_eq!(17, find_ident_end("let an_identifier = 100;", 4));
}

#[test]
fn find_ident_end_unicode() {
    assert_eq!(7, find_ident_end("num_µs", 0));
    assert_eq!(10, find_ident_end("ends_in_µ", 0));
}

// PD: short term replacement for .char_at() function. Should be replaced once
// that stabilizes
pub fn char_at(src: &str, i: usize) -> char {
    src[i..].chars().next().unwrap()
}

/// Error type returned from [`check_rust_src_env_var()`]
///
/// [`check_rust_src_env_var()`]: fn.check_rust_src_env_var.html
#[derive(Debug)]
pub enum RustSrcPathError {
    Missing,
    DoesNotExist(path::PathBuf),
    NotRustSourceTree(path::PathBuf),
}

impl ::std::error::Error for RustSrcPathError {
    fn cause(&self) -> Option<&::std::error::Error> {
        None
    }

    fn description(&self) -> &str {
        match *self {
            RustSrcPathError::Missing => "RUSTC_SRC_PATH not set or not found in sysroot",
            RustSrcPathError::DoesNotExist(_) => "RUSTC_SRC_PATH does not exist on file system",
            RustSrcPathError::NotRustSourceTree(_) => "RUSTC_SRC_PATH isn't a rustc source tree",
        }
    }
}

impl ::std::fmt::Display for RustSrcPathError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match *self {
            RustSrcPathError::Missing => {
                write!(f, "RUST_SRC_PATH environment variable must be set to \
                           point to the src directory of a rust checkout. \
                           E.g. \"/home/foouser/src/rust/src\"")
            },
            RustSrcPathError::DoesNotExist(ref path) => {
                write!(f, "racer can't find the directory pointed to by the \
                           RUST_SRC_PATH variable \"{:?}\". Try using an \
                           absolute fully qualified path and make sure it \
                           points to the src directory of a rust checkout - \
                           e.g. \"/home/foouser/src/rust/src\".", path)
            },
            RustSrcPathError::NotRustSourceTree(ref path) => {
                write!(f, "Unable to find libstd under RUST_SRC_PATH. N.B. \
                           RUST_SRC_PATH variable needs to point to the *src* \
                           directory inside a rust checkout e.g. \
                           \"/home/foouser/src/rust/src\". \
                           Current value \"{:?}\"", path)
            },
        }
    }
}

fn check_rust_sysroot() -> Option<path::PathBuf> {
    use ::std::process::Command;
    let mut cmd = Command::new("rustc");
    cmd.arg("--print").arg("sysroot");

    if let Ok(output) = cmd.output() {
        if let Ok(s) = String::from_utf8(output.stdout) {
            let sysroot = path::Path::new(s.trim());
            let srcpath = sysroot.join("lib/rustlib/src/rust/src");
            if srcpath.exists() {
                return Some(srcpath);
            }
        }
    }
    None
}

/// Check for `RUST_SRC_PATH` environment variable validity and presence
///
/// If the environment variable is _not_ set, try and set it from the rust sys
/// root.
///
/// If the rust src path is there and valid or can be set, Ok(()) is returned.
/// Otherwise, an error with the appropriate reason is provided.
///
/// # Examples
///
/// ```
/// extern crate racer;
///
/// match racer::check_rust_src_env_var() {
///     Ok(()) => {
///         // RUST_SRC_PATH is valid
///     },
///     Err(racer::RustSrcPathError::Missing) => {
///         // path is not set or not found in sysroot
///     },
///     Err(racer::RustSrcPathError::DoesNotExist(_path)) => {
///         // provided path doesnt point to valid file
///     },
///     Err(racer::RustSrcPathError::NotRustSourceTree(_path)) => {
///         // provided path doesn't have rustc src
///     }
/// }
/// ```
pub fn check_rust_src_env_var() -> ::std::result::Result<(), RustSrcPathError> {
    use std::env;
    use nameres;

    match env::var("RUST_SRC_PATH") {
        Ok(ref srcpaths) if !srcpaths.is_empty() => {
            // TODO implementation has the same behavior as the original
            // (before returning an error) where only the first path in
            // RUST_SRC_PATH is considered. This should either expect a single
            // path to be provided, or all paths should be considered. The
            // latter option would need to be supported in the rest of racer.

            // Unwrap is ok here since split returns the original string
            // even if it doesn't contain the split pattern.
            let v = srcpaths.split(nameres::PATH_SEP).next().unwrap();
            let f = path::Path::new(v);
            if !f.exists() {
                Err(RustSrcPathError::DoesNotExist(f.to_path_buf()))
            } else if !f.join("libstd").exists() {
                Err(RustSrcPathError::NotRustSourceTree(f.join("libstd")))
            } else {
                Ok(())
            }
        },
        _ => {
            if let Some(path) = check_rust_sysroot() {
                env::set_var("RUST_SRC_PATH", path);
                Ok(())
            } else {
                let default_paths = [
                    "/usr/local/src/rust/src",
                    "/usr/src/rust/src",
                ];

                for &path in &default_paths {
                    let f = path::Path::new(path);
                    if f.exists() {
                        env::set_var("RUST_SRC_PATH", path);
                        return Ok(())
                    }
                }

                Err(RustSrcPathError::Missing)
            }
        }
    }
}

/// An immutable stack implemented as a linked list backed by a thread's stack.
pub struct StackLinkedListNode<'stack, T>(Option<StackLinkedListNodeData<'stack, T>>)
    where T: 'stack;

struct StackLinkedListNodeData<'stack, T>
    where T: 'stack
{
    item: T,
    previous: &'stack StackLinkedListNode<'stack, T>,
}

impl<'stack, T> StackLinkedListNode<'stack, T>
    where T: 'stack
{
    /// Returns an empty node.
    pub fn empty() -> Self {
        StackLinkedListNode(None)
    }

    /// Pushes a new node on the stack. Returns the new node.
    pub fn push(&'stack self, item: T) -> Self {
        StackLinkedListNode(Some(StackLinkedListNodeData {
            item: item,
            previous: self,
        }))
    }
}

impl<'stack, T> StackLinkedListNode<'stack, T>
    where T: 'stack + PartialEq
{
    /// Check if the stack contains the specified item.
    /// Returns `true` if the item is found, or `false` if it's not found.
    pub fn contains(&self, item: &T) -> bool {
        let mut current = self;
        while let &StackLinkedListNode(Some(StackLinkedListNodeData { item: ref current_item, previous })) = current {
            if current_item == item {
                return true;
            }

            current = previous;
        }

        false
    }
}

/// Removes `pub(...)` from the start of a blob so that other code
/// can assess the struct/trait/fn without worrying about restricted
/// visibility.
pub fn trim_visibility(blob: &str) -> &str {
    if !blob.trim_left().starts_with("pub") {
        return blob
    }
    
    let mut level = 0;
    let mut skip_restricted = 0;
    for (i, c) in blob[3..].char_indices() {
        match c {
            '(' => level += 1,
            ')' => level -= 1,
            _ if level >= 1 => (),
            // stop on the first thing that isn't whitespace
            _ if is_ident_char(c) => {
                skip_restricted = i + 3;
                break;
            },
            _ => continue,
        }
    }

    &blob[skip_restricted..]
}

#[test]
fn test_trim_visibility() {
    assert_eq!(trim_visibility("pub fn"), "fn");
    assert_eq!(trim_visibility("pub(crate)   struct"), "struct");
    assert_eq!(trim_visibility("pub (in super)  const fn"), "const fn");
}

fn is_after_keyword(keyword: &str, line_before_point: &str) -> bool {
    // Determine if the cursor is sitting in the whitespace after typing `[keyword] ` before
    // typing a name.
    let has_started_name = !line_before_point.ends_with(|c: char| c.is_whitespace());

    let mut words = line_before_point.split_whitespace().rev();

    // Make sure we haven't finished the name and started generics or arguments
    if has_started_name {
        if let Some(ident) = words.next() {
            if ident.chars().any(|c| !is_ident_char(c)) {
                return false;
            }
        }
    }
    
    words
        .next()
        .map(|word| word == keyword)
        .unwrap_or_default()
}

/// Checks if the completion point is in a function declaration by looking
/// to see if the second-to-last word is `fn`.
pub fn in_fn_name(line_before_point: &str) -> bool {
    is_after_keyword("fn", line_before_point)
}

#[test]
fn test_in_fn_name() {
    assert!(in_fn_name("fn foo"));
    assert!(in_fn_name(" fn  foo"));
    assert!(in_fn_name("fn "));
    assert!(!in_fn_name("fn foo(b"));
    assert!(!in_fn_name("fn"));
}

/// Checks if the completion point is in a type or associated type declaration
/// by looking to see if the second-to-last word is `type`.
pub fn in_type_name(line_before_point: &str) -> bool {
    is_after_keyword("type", line_before_point)
}

#[test]
fn test_in_type_name() {
    assert!(in_type_name("type Er"));
    assert!(in_type_name(" type  Err"));


    assert!(!in_type_name("type Foo<T"));
    assert!(!in_type_name("type Foo=String"));
    assert!(!in_type_name("type Foo = String"));
}