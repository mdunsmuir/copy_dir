//! The essential objective of this crate is to provide an API for copying
//! directories and their contents in a straightforward and predictable way.
//! See the documentation of the `copy_dir` function for more info.

#[macro_use]
extern crate log;

use std::io;
use std::fs;
use std::path::{Path, PathBuf};

// TODO macro this block for portability
use std::os::unix::fs::MetadataExt;

type UniqueId = (u64, u64);

fn new_os_file<P: AsRef<Path>>(path: P) -> Box<OsFile<UniqueId=UniqueId>> {
    Box::new(LunixFile { path: path.as_ref().to_path_buf() })
}
// TODO macro above

#[derive(Debug)]
pub enum Error {
    DestinationExists {
        source: PathBuf,
        destination: PathBuf
    },
    SourceDoesNotExist(PathBuf),
    SourceIsDestinationRoot {
        source: PathBuf,
        destination: PathBuf
    },
    Unknown(PathBuf),
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// This can be used to specify the error reporting behavior of the 
/// `copy_dir_with_handler` function.
#[derive(Debug)]
pub enum Handler {
    /// Supply a vector that will be filled with errors
    Vector(Vec<Error>),

    /// Log errors, as per the `log` crate (client must initialize their
    /// own logger object)
    Log,
    ///
    /// Silently swallow errors
    Ignore,
}

impl Handler {
    fn handle(&mut self, error: Error) {
        match *self {
            Handler::Vector(ref mut vec) => vec.push(error),
            Handler::Log => error!("{:?}", error),
            _ => (),
        }
    }
}

macro_rules! handle {
    ($handler:expr, $expr:expr) => {
        match $expr {
            Err(err) => {
                $handler.handle(Error::from(err));
                return;
            },
            Ok(value) => value,
        }
    }
}

trait OsFile {
    type UniqueId;

    fn path(&self) -> &Path;
    fn unique_id(&self) -> Result<UniqueId>;
    fn copy(&self,
            destination: &Path,
            root_destination: Option<Self::UniqueId>,
            error_handler: &mut Handler);

    fn metadata(&self) -> Result<std::fs::Metadata> {
        std::fs::metadata(&self.path())
            .map_err( |err| Error::from(err) )
    }
}

struct LunixFile {
    path: PathBuf,
}

impl OsFile for LunixFile {
    type UniqueId = (u64, u64); // dev and inode

    fn path(&self) -> &Path {
        self.path.as_ref()
    }

    // TODO macro in different variants here for linux/unix
    fn unique_id(&self) -> Result<Self::UniqueId> {
        let metadata = self.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    fn copy(&self,
            destination: &Path,
            mut root_destination: Option<UniqueId>,
            handler: &mut Handler) {

        let unique_id = handle!(handler, self.unique_id());
        let metadata = handle!(handler, self.metadata());

        if metadata.is_file() {
            handle!(
                handler,
                fs::copy(&self.path, destination).map( |_| () )
                    .map_err( |err| Error::from(err) )
            )

        } else if metadata.is_dir() {
            // if this hasn't been set yet, then this must be the root of
            // the copy, and therefore we can set it to the current
            // destination
            if let None = root_destination {
                root_destination = Some(unique_id);

            // we ignore the root of the new copy so we don't recursively copy
            // forever or until computer gets sad
            } else if unique_id == root_destination.unwrap() {
                handle!(
                    handler,
                    Err(Error::SourceIsDestinationRoot {
                        source: self.path.clone(),
                        destination: destination.to_path_buf(),
                    })
                );
                return
            }

            handle!(
                handler,
                fs::create_dir_all(destination)
            );

            for entry in handle!(handler, fs::read_dir(&self.path)) {
                let entry = handle!(handler, entry);

                LunixFile {
                    path: entry.path()
                }.copy(
                    &destination.join(entry.file_name()),
                    root_destination,
                    handler
                );
            }

            // do this last just to avoid any weirdness during the copy
            // probably totally unnecessary, but why not?
            handle!(
                handler,
                fs::set_permissions(destination, metadata.permissions())
            );

        } else {
            handle!(
                handler,
                Err(Error::Unknown(self.path.clone()))
            )
        }
    }

    // TODO override metadata method to cache it
}

/// Copy a directory and its contents
///
/// The file or directory at the source path is copied
/// to the destination path. If the source path points to a directory, it will
/// be copied recursively with its contents.
///
/// # Errors
///
/// * It's possible for many errors to occur during the recursive copy
///   operation. These errors are all returned in a `Vec`. They may or may
///   not be helpful or useful.
/// * If the source path does not exist.
/// * If the destination path exists.
/// * If something goes wrong with copying a regular file, as with
///   `std::fs::copy()`.
/// * If something goes wrong creating the new root directory when copying
///   a directory, as with `std::fs::create_dir()`.
/// * If you try to copy a directory to a path prefixed by itself e.g.
///   `copy_dir(".", "./foo")`. See below for more details.
///
/// # Caveats/Limitations
///
/// I would like to add some flexibility around how "edge cases" in the copying
/// operation are handled, but for now there is no flexibility and the following
/// caveats and limitations apply (not by any means an exhaustive list):
///
/// * You cannot currently copy a directory into itself i.e.
///   `copy_dir(".", "./foo")`. This is because we are recursively walking
///   the directory to be copied *while* we're copying it, so in this edge
///   case you get an infinite recursion. Fixing this is the top of my list
///   of things to do with this crate.
/// * Hard links are not accounted for, i.e. if more than one hard link
///   pointing to the same inode are to be copied, the data will be copied
///   twice.
/// * Filesystem boundaries may be crossed.
/// * Symbolic links will be copied, not followed.
pub fn copy_dir<Q, P>(from: P, to: Q) -> Result<()>
    where Q: AsRef<Path>, P: AsRef<Path> {

    copy_dir_with_handler(from, to, &mut Handler::Ignore)
}

/// Same as copy_dir, but allows clients to specify a `Handler` for any errors
/// that occur. 
pub fn copy_dir_with_handler<Q, P>(from: P, to: Q,
                                   handler: &mut Handler) -> Result<()>
    where Q: AsRef<Path>, P: AsRef<Path> {

    if !from.as_ref().exists() {
        return Err(Error::SourceDoesNotExist(from.as_ref().to_path_buf()));

    } else if to.as_ref().exists() {
        return Err(Error::DestinationExists {
            source: from.as_ref().to_path_buf(),
            destination: to.as_ref().to_path_buf(),
        });
    }

    let source = new_os_file(&from);
    source.copy(to.as_ref(), None, handler);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(unused_variables)]

    extern crate std;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    extern crate walkdir;

    use super::Error;

    extern crate fs_test_helpers;
    use self::fs_test_helpers::{
        TempDir,
        Fake,
        assert_files_have_same_contents,
    };

    #[test]
    fn single_file() {
        let file = Fake::file("foo.file");
        assert_we_match_the_real_thing(&file, true, None);
    }

    #[test]
    fn directory_with_file() {
        let dir = Fake::dir("foo", vec![
            Fake::file("bar").fill_with_uuid(),
            Fake::dir("baz", vec![
                Fake::file("quux").fill_with_uuid(),
                Fake::file("fobe").fill_with_uuid()
            ])
        ]);
        assert_we_match_the_real_thing(&dir, true, None);
    }

    #[test]
    fn source_does_not_exist() {
        let base_dir = TempDir::new("copy_dir_test").unwrap();
        let source_path = base_dir.as_ref().join("noexist.file");
        match super::copy_dir(&source_path, "dest.file") {
            Ok(_) => panic!("expected Err"),
            Err(err) => match err {
                Error::SourceDoesNotExist { .. } => (),
                _ => panic!("expected SourceDoesNotExist"),
            },
        }
    }

    #[test]
    fn target_exists() {
        let base_dir = TempDir::new("copy_dir_test").unwrap();
        let source_path = base_dir.as_ref().join("exist.file");
        let target_path = base_dir.as_ref().join("exist2.file");

        {
            fs::File::create(&source_path).unwrap();
            fs::File::create(&target_path).unwrap();
        }

        match super::copy_dir(&source_path, &target_path) {
            Ok(_) => panic!("expected Err"),
            Err(err) => match err {
                Error::DestinationExists { .. } => (),
                _ => panic!("expected kind AlreadyExists")
            }
        }
    }

    #[test]
    fn attempt_copy_under_self() {
        let base_dir = TempDir::new("copy_dir_test").unwrap();
        let dir = Fake::dir("foo", vec![
            Fake::file("bar"),
            Fake::dir("baz", vec![
                Fake::file("quux"),
                Fake::file("fobe")
            ])
        ]);
        dir.create(&base_dir).unwrap();

        let from = base_dir.as_ref().join("foo");
        let to = from.as_path().join("beez");

        let copy_result = super::copy_dir(&from, &to).unwrap();
    }

    fn assert_dirs_same<P: AsRef<Path>>(a: P, b: P) {
        let mut wa = walkdir::WalkDir::new(a.as_ref()).into_iter();
        let mut wb = walkdir::WalkDir::new(b.as_ref()).into_iter();

        loop {
            let o_na = wa.next();
            let o_nb = wb.next();

            if o_na.is_some() && o_nb.is_some() {
                let r_na = o_na.unwrap();
                let r_nb = o_nb.unwrap();

                if r_na.is_ok() && r_nb.is_ok() {
                    let na = r_na.unwrap();
                    let nb = r_nb.unwrap();

                    assert_eq!(
                        na.path().strip_prefix(a.as_ref()),
                        nb.path().strip_prefix(b.as_ref())
                    );

                    assert_eq!(na.file_type(), nb.file_type());

                    if na.file_type().is_file() {
                        assert_files_have_same_contents(
                            &na.path(),
                            &nb.path(),
                        )
                    }

                    // TODO test permissions
                }

            } else if o_na.is_none() && o_nb.is_none() {
                return
            } else {
                assert!(false);
            }
        }
    }

    fn assert_we_match_the_real_thing(dir: &Fake,
                                      explicit_name: bool,
                                      o_pre_state: Option<&Fake>) {
        let base_dir = TempDir::new("copy_dir_test").unwrap();

        let source_dir = base_dir.as_ref().join("source");
        let our_dir = base_dir.as_ref().join("ours");
        let their_dir = base_dir.as_ref().join("theirs");

        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&our_dir).unwrap();
        fs::create_dir(&their_dir).unwrap();

        dir.create(&source_dir).unwrap();
        let source_path = source_dir.as_path().join(dir.name());

        let (our_target, their_target) = if explicit_name {
            (
                our_dir.as_path().join(dir.name()),
                their_dir.as_path().join(dir.name())
            )
        } else {
            (our_dir.clone(), their_dir.clone())
        };

        if let Some(pre_state) = o_pre_state {
            pre_state.create(&our_dir).unwrap();
            pre_state.create(&their_dir).unwrap();
        }

        let we_good = super::copy_dir(&source_path, &our_target).is_ok();

        let their_status = Command::new("cp")
            .arg("-r")
            .arg(source_path.as_os_str())
            .arg(their_target.as_os_str())
            .status()
            .unwrap();

        // TODO any way to ask cp whether it worked or not?
        // portability?
        // assert_eq!(we_good, their_status.success());
        assert_dirs_same(&their_dir, &our_dir);
    }

    #[test]
    fn dir_maker_and_assert_dirs_same_baseline() {
        let dir = Fake::dir(
            "foobar",
            vec![
                Fake::file("bar"),
                Fake::dir("baz", Vec::new())
            ]
        );

        let base_dir = TempDir::new("copy_dir_test").unwrap();

        let a_path = base_dir.as_ref().join("a");
        let b_path = base_dir.as_ref().join("b");

        fs::create_dir(&a_path).unwrap();
        fs::create_dir(&b_path).unwrap();

        dir.create(&a_path).unwrap();
        dir.create(&b_path).unwrap();

        assert_dirs_same(&a_path, &b_path);
    }

    #[test]
    #[should_panic]
    fn assert_dirs_same_properly_fails() {
        let dir = Fake::dir(
            "foobar",
            vec![
                Fake::file("bar"),
                Fake::dir("baz", Vec::new())
            ]
        );

        let dir2 = Fake::dir(
            "foobar",
            vec![
                Fake::file("fobe"),
                Fake::file("beez")
            ]
        );

        let base_dir = TempDir::new("copy_dir_test").unwrap();

        let a_path = base_dir.as_ref().join("a");
        let b_path = base_dir.as_ref().join("b");

        fs::create_dir(&a_path).unwrap();
        fs::create_dir(&b_path).unwrap();

        dir.create(&a_path).unwrap();
        dir2.create(&b_path).unwrap();

        assert_dirs_same(&a_path, &b_path);
    }

}
