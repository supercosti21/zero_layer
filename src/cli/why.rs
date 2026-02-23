//! `zl why <package>` — show why a package is installed (dependency chain).

use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};

use super::WhyArgs;

pub fn handle(args: WhyArgs, db: &ZlDatabase) -> ZlResult<()> {
    let pkg = db
        .get_package_by_name(&args.package)?
        .ok_or_else(|| ZlError::PackageNotFound {
            name: args.package.clone(),
        })?;

    if pkg.explicit {
        println!(
            "{}-{} was explicitly installed by the user.",
            pkg.id.name, pkg.id.version
        );
        return Ok(());
    }

    // Find reverse dependency chain
    println!(
        "{}-{} is installed as a dependency.\n",
        pkg.id.name, pkg.id.version
    );

    let chain = find_dependency_chain(&args.package, db, 0)?;
    if chain.is_empty() {
        println!(
            "  No reverse dependency found — this may be an orphan.\n  hint: remove it with `zl remove {}`",
            args.package
        );
    }

    Ok(())
}

/// Recursively trace why a package is installed, printing the chain.
fn find_dependency_chain(
    package_name: &str,
    db: &ZlDatabase,
    depth: usize,
) -> ZlResult<Vec<String>> {
    let mut chain = Vec::new();
    let indent = "  ".repeat(depth);

    let rdeps = db.reverse_dependencies(package_name)?;

    if rdeps.is_empty() {
        return Ok(chain);
    }

    for rdep_key in &rdeps {
        // rdep_key is "name-version"; extract name
        let rdep_name = rdep_key
            .rfind('-')
            .map(|pos| &rdep_key[..pos])
            .unwrap_or(rdep_key);

        if let Some(rdep_node) = db.get_package_by_name(rdep_name)? {
            if rdep_node.explicit {
                println!(
                    "{}-> {}-{} (explicitly installed)",
                    indent, rdep_node.id.name, rdep_node.id.version
                );
                chain.push(rdep_name.to_string());
            } else {
                println!(
                    "{}-> {}-{} (dependency)",
                    indent, rdep_node.id.name, rdep_node.id.version
                );
                chain.push(rdep_name.to_string());
                // Avoid infinite recursion
                if depth < 10 {
                    let sub_chain = find_dependency_chain(rdep_name, db, depth + 1)?;
                    chain.extend(sub_chain);
                }
            }
        } else {
            println!("{}-> {} (not found in DB)", indent, rdep_key);
        }
    }

    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_dependency_chain_empty() {
        let db_file = tempfile::NamedTempFile::new().unwrap();
        let db = ZlDatabase::open(db_file.path()).unwrap();

        let chain = find_dependency_chain("nonexistent", &db, 0).unwrap();
        assert!(chain.is_empty());
    }
}
