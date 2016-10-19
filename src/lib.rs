extern crate walkdir;

use std::fs;
use std::path::{Path, PathBuf};
use std::io::{Error, ErrorKind};

macro_rules! push_error {
    ($expr:expr, $vec:ident) => {
        match $expr {
            Err(e) => $vec.push(e),
            Ok(_) => (),
        }
    }
}

pub fn cp_r<Q: AsRef<Path>, P: AsRef<Path>>(from: P, to: Q)
                                            -> std::io::Result<Vec<Error>> {
    {
        let source_metadata = try!(fs::metadata(&from));

        // if the source file is not a directory, then we'll just copy it
        // regular-like. I think this is what real cp does.
        if !source_metadata.is_dir() {
            let real_to = try!(actual_target(&from, &to, false));
            return fs::copy(&from, &real_to).map(|_| Vec::new() );
        }
    }

    let real_to = try!(actual_target(&from, &to, true));
    let mut errors = Vec::new();

    for entry in walkdir::WalkDir::new(&from)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok() ) {

        let relative_path = match entry.path().strip_prefix(&from) {
            Ok(rp) => rp,
            Err(_) => panic!("strip_prefix failed; this is a probably a bug in cp_r"),
        };

        let target_path = {
            let mut target_path = real_to.clone();
            target_path.push(relative_path);
            target_path
        };

        let source_metadata = match entry.metadata() {
            Err(_) => {
                errors.push(Error::new(
                    ErrorKind::Other,
                    format!("walkdir metadata error for {:?}", entry.path())
                ));

                continue
            },

            Ok(md) => md,
        };

        if source_metadata.is_dir() {
            push_error!(fs::create_dir(&target_path), errors);
            push_error!(
                fs::set_permissions(&target_path, source_metadata.permissions()),
                errors
            );
        } else {
            push_error!(fs::copy(entry.path(), &target_path), errors);
        }
    }

    Ok(errors)
}

fn actual_target<Q: AsRef<Path>, P: AsRef<Path>>(from: P, to: Q,
                                                 create_dir: bool)
                                                 -> std::io::Result<PathBuf> {
    match fs::metadata(&to) {
        // if there is nothing at the target path, we create a directory, EZ
        Err(ref err) if err.kind() == ErrorKind::NotFound => {
            if create_dir { try!(fs::create_dir(&to)); }
            Ok(to.as_ref().to_path_buf())
        },

        Err(err) => return Err(err),

        // if there is something, ...
        Ok(md) => {

            // if it's a directory, we'll create a new directory underneath
            // it with the same basename as the source (this is what real cp
            // does) and that'll be the target of the copy.
            if md.is_dir() {
                match from.as_ref().file_name() {
                    None => return Err(Error::new(
                        ErrorKind::Other,
                        "could not get basename of source path"
                    )),

                    Some(basename) => {
                        let mut real_to = to.as_ref().to_path_buf();
                        real_to.push(basename);

                        match fs::metadata(&real_to) {
                            Err(ref err) if err.kind() == ErrorKind::NotFound =>
                                if create_dir { try!(fs::create_dir(&real_to)) },

                            Err(err) => return Err(err),

                            Ok(md) => if !md.is_dir() {
                                return Err(Error::new(
                                    ErrorKind::Other,
                                    format!("{:?} is not a directory", real_to)
                                ))
                            }
                        }
                        Ok(real_to)
                    },
                }

            // if it's not a directory, we can't do anything
            // this is also what real cp does
            } else {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    "target exists and is not a directory"
                ))
            }
        },
    }
}


#[cfg(test)]
mod tests {
    #![allow(unused_variables)]

    extern crate std;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    extern crate walkdir;
    extern crate tempdir;

    #[test]
    fn single_file() {
        let file = File("foo.file");
        assert_we_match_the_real_thing(&file, false, None);
        assert_we_match_the_real_thing(&file, true, None);
    }

    #[test]
    fn single_file_implicit_into_directory() {
        let file = File("foo");
        let already_there = Dir("foo", Vec::new());
        assert_we_match_the_real_thing(&file, true, Some(&already_there));
    }

    #[test]
    fn directory_with_file() {
        let dir = Dir("foo", vec![
            File("bar")
        ]);
        assert_we_match_the_real_thing(&dir, false, None);
        assert_we_match_the_real_thing(&dir, true, None);
    }

    #[test]
    fn directory_with_file_implicit_into_directory() {
        let dir = Dir("foo", vec![
            File("bar")
        ]);
        let already_there = Dir("foo", Vec::new());
        assert_we_match_the_real_thing(&dir, true, Some(&already_there));
    }

    #[test]
    fn directory_file_already_there() {
        let dir = Dir("foo", vec![
            File("bar")
        ]);
        let already_there = File("foo");
        //assert_we_match_the_real_thing(&dir, false, Some(&already_there));
        assert_we_match_the_real_thing(&dir, true, Some(&already_there));
    }

    // utility stuff below here

    enum DirMaker<'a> {
        Dir(&'a str, Vec<DirMaker<'a>>),
        File(&'a str),
    }

    use self::DirMaker::*;

    impl<'a> DirMaker<'a> {
        fn create<P: AsRef<Path>>(&self, base: P) -> std::io::Result<()> {
            match *self {
                Dir(ref name, ref contents) => {
                    let path = base.as_ref().join(name);
                    try!(fs::create_dir(&path));

                    for thing in contents {
                        try!(thing.create(&path));
                    }
                },

                File(ref name) => {
                    let path = base.as_ref().join(name);
                    try!(fs::File::create(path));
                }
            }

            Ok(())
        }

        fn name(&self) -> &str {
            match *self {
                Dir(name, _) => name,
                File(name) => name,
            }
        }
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

                    // TODO test permissions
                }

            } else if o_na.is_none() && o_nb.is_none() {
                return
            } else {
                assert!(false);
            }
        }
    }

    fn assert_we_match_the_real_thing(dir: &DirMaker,
                                      explicit_name: bool,
                                      o_pre_state: Option<&DirMaker>) {
        let base_dir = tempdir::TempDir::new("cp_r_test").unwrap();

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

        let we_good = super::cp_r(&source_path, &our_target).is_ok();

        let their_status = Command::new("cp")
            .arg("-r")
            .arg(source_path.as_os_str())
            .arg(their_target.as_os_str())
            .status()
            .unwrap();

        let tree_output = Command::new("tree")
            .arg(base_dir.as_ref().as_os_str())
            .output()
            .unwrap();

        println!("{}",
                 std::str::from_utf8(tree_output.stdout.as_slice()).unwrap());

        // TODO any way to ask cp whether it worked or not?
        // portability?
        // assert_eq!(we_good, their_status.success());
        assert_dirs_same(&their_dir, &our_dir);
    }

    #[test]
    fn dir_maker_and_assert_dirs_same_baseline() {
        let dir = Dir(
            "foobar",
            vec![
                File("bar"),
                Dir("baz", Vec::new())
            ]
        );

        let base_dir = tempdir::TempDir::new("cp_r_test").unwrap();

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
        let dir = Dir(
            "foobar",
            vec![
                File("bar"),
                Dir("baz", Vec::new())
            ]
        );

        let dir2 = Dir(
            "foobar",
            vec![
                File("fobe"),
                File("beez")
            ]
        );

        let base_dir = tempdir::TempDir::new("cp_r_test").unwrap();

        let a_path = base_dir.as_ref().join("a");
        let b_path = base_dir.as_ref().join("b");

        fs::create_dir(&a_path).unwrap();
        fs::create_dir(&b_path).unwrap();

        dir.create(&a_path).unwrap();
        dir2.create(&b_path).unwrap();

        assert_dirs_same(&a_path, &b_path);
    }

}
