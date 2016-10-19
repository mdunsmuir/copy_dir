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
            return fs::copy(from, to).map(|_| Vec::new() );
        }
    }

    let mut errors = Vec::new();
    let real_to = try!(actual_target(&from, &to));

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

fn actual_target<Q: AsRef<Path>, P: AsRef<Path>>(from: P, to: Q)
                                                 -> std::io::Result<PathBuf> {
    match fs::metadata(&to) {
        // if there is nothing at the target path, we create a directory, EZ
        Err(ref err) if err.kind() == ErrorKind::NotFound => {
            try!(fs::create_dir(&to));
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
                                try!(fs::create_dir(&real_to)),

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

    #[test]
    fn foo() {
        println!("{:?}", super::cp_r("/Users/mdunsmuir/projects/dredge/src", "foo"));
    }
}
